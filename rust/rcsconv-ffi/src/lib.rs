//! C ABI for the Revit add-in (and any other host): build a `.rcs` file.
//!
//! All strings are UTF-8, NUL-terminated. Every pointer returned by this crate
//! must be released with [`rcsconv_free`]. Panics never cross the boundary.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use rcs_conv::{convert_one, convert_to, Conversion, NoProgress, Options, Status};

/// Bit 0: overwrite an existing `.rcs`; bit 1: use the in-memory reference writer.
const FLAG_FORCE: u32 = 1;
const FLAG_IN_MEMORY: u32 = 2;

#[no_mangle]
pub extern "C" fn rcsconv_version() -> *mut c_char {
    to_ptr(env!("CARGO_PKG_VERSION").to_string())
}

/// Free a string returned by this crate. Passing NULL is allowed.
#[no_mangle]
pub extern "C" fn rcsconv_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(text));
    }
}

/// Convert one file into a directory; the name is derived from the input.
///
/// Returns a NUL-terminated UTF-8 response; release it with [`rcsconv_free`].
/// The response is line based: `ok`, output path, points, tiles, depth; or
/// `err` followed by the message.
#[no_mangle]
pub extern "C" fn rcsconv_convert(
    input: *const c_char,
    out_dir: *const c_char,
    guid: *const c_char,
    max_per_tile: u32,
    flags: u32,
) -> *mut c_char {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        convert(input, out_dir, guid, max_per_tile, flags)
    }));
    match outcome {
        Ok(response) => to_ptr(response),
        Err(_) => to_ptr(to_error("internal panic")),
    }
}

/// Convert one file into an explicit target path (the GUI "Save as" flow).
#[no_mangle]
pub extern "C" fn rcsconv_convert_to(
    input: *const c_char,
    target: *const c_char,
    guid: *const c_char,
    max_per_tile: u32,
    flags: u32,
) -> *mut c_char {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        convert_into(input, target, guid, max_per_tile, flags)
    }));
    match outcome {
        Ok(response) => to_ptr(response),
        Err(_) => to_ptr(to_error("internal panic")),
    }
}

fn convert(
    input: *const c_char,
    out_dir: *const c_char,
    guid: *const c_char,
    max_per_tile: u32,
    flags: u32,
) -> String {
    let input = match read_string(input) {
        Some(text) => text,
        None => return to_error("input path is empty"),
    };
    let out_dir = match read_string(out_dir) {
        Some(text) => text,
        None => return to_error("out_dir is empty"),
    };
    let guid = read_string(guid);
    if max_per_tile == 0 || max_per_tile > 65536 {
        return to_error(&format!("max_per_tile={max_per_tile} out of 1..65536"));
    }

    let options = Options {
        sources: vec![input.clone()],
        out_dir: Some(PathBuf::from(&out_dir)),
        max_per_tile,
        force: flags & FLAG_FORCE != 0,
        recursive: false,
        guid,
        las: Default::default(),
        in_memory: flags & FLAG_IN_MEMORY != 0,
    };
    if let Err(message) = options.validate() {
        return to_error(&message);
    }
    let build = match options.build_options() {
        Ok(build) => build,
        Err(message) => return to_error(&message),
    };

    let path = Path::new(&input);
    let las = is_las(path);
    let result = convert_one(path, &options, &build, las, &NoProgress);
    response(&result)
}

fn convert_into(
    input: *const c_char,
    target: *const c_char,
    guid: *const c_char,
    max_per_tile: u32,
    flags: u32,
) -> String {
    let input = match read_string(input) {
        Some(text) => text,
        None => return to_error("input path is empty"),
    };
    let target = match read_string(target) {
        Some(text) => text,
        None => return to_error("target path is empty"),
    };
    let guid = read_string(guid);
    if max_per_tile == 0 || max_per_tile > 65536 {
        return to_error(&format!("max_per_tile={max_per_tile} out of 1..65536"));
    }

    let options = Options {
        sources: vec![input.clone()],
        out_dir: None,
        max_per_tile,
        force: flags & FLAG_FORCE != 0,
        recursive: false,
        guid,
        las: Default::default(),
        in_memory: flags & FLAG_IN_MEMORY != 0,
    };
    if let Err(message) = options.validate() {
        return to_error(&message);
    }
    let build = match options.build_options() {
        Ok(build) => build,
        Err(message) => return to_error(&message),
    };

    let path = Path::new(&input);
    let las = is_las(path);
    let result = convert_to(path, Path::new(&target), &options, &build, las, &NoProgress);
    response(&result)
}

fn is_las(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("las") || value.eq_ignore_ascii_case("laz"))
        .unwrap_or(false)
}

fn response(result: &Conversion) -> String {
    match result.status {
        Status::Written | Status::Skipped => {
            let stats = result.stats.as_ref();
            let points = stats.map(|value| value.points).unwrap_or(0);
            let tiles = stats.map(|value| value.tiles).unwrap_or(0);
            let depth = stats.map(|value| value.depth).unwrap_or(0);
            format!(
                "ok\n{}\n{points}\n{tiles}\n{depth}",
                result.target.display()
            )
        }
        _ => to_error(&result.message()),
    }
}

fn to_error(message: &str) -> String {
    format!("err\n{message}")
}

fn read_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(pointer) };
    value.to_str().ok().map(|text| text.to_string())
}

fn to_ptr(text: String) -> *mut c_char {
    match CString::new(text) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new("err\ntext contains NUL").unwrap().into_raw(),
    }
}
