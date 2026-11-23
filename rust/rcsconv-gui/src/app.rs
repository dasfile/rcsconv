//! Main window: class registration, file queue, message loop.
//!
//! Phases G2-G4 (`docs/ROADMAP.md` 4.1): file list (`SysListView32`), file and
//! directory dialogs, drag and drop, Remove/Clear, conversion parameters and the
//! background conversion job.

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_sys::Win32::UI::Controls::{
    LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, PBS_SMOOTH,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::job::{self, Shared};
use crate::model::{self, EntryStatus, Id, Queue};
use crate::params::{RecenterChoice, Settings};
use crate::win32;

/// `COLOR_WINDOW` (5) + 1: this is how Win32 asks for the system window brush.
const BACKGROUND_BRUSH: u32 = 5 + 1;

/// Timer id used to poll the background thread.
const TIMER_ID: usize = 1;

/// Poll period, ms.
const TIMER_PERIOD: u32 = 120;

/// File dialog filter.
const OPEN_FILTER: [(&str, &str); 2] =
    [("Point clouds", "*.las;*.laz;*.pts"), ("All files", "*.*")];

/// Window state: lives in `GWLP_USERDATA` of the main window.
struct State {
    queue: Queue,
    /// Background job (while a conversion is running).
    job: Option<Job>,
    /// How many results are already shown in the list.
    applied: usize,
    /// Summary of the last run (shown in the hint).
    summary: Option<String>,
}

/// Background job.
struct Job {
    shared: Arc<Shared>,
    handle: JoinHandle<()>,
}

impl State {
    fn new() -> Box<State> {
        Box::new(State { queue: Queue::new(), job: None, applied: 0, summary: None })
    }
}

/// Start the interface; return the process exit code.
pub fn run() -> u8 {
    win32::enable_dpi_awareness();
    win32::init_common_controls();

    let instance = win32::instance();
    let class_name = win32::wide(model::WINDOW_CLASS);
    let title = win32::wide(model::WINDOW_TITLE);

    // SAFETY: WNDCLASSW is a plain struct; we fill the needed fields, the rest stay zero.
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.style = CS_HREDRAW | CS_VREDRAW;
    class.lpfnWndProc = Some(wndproc);
    class.hInstance = instance;
    // SAFETY: IDC_ARROW is the standard cursor; hInstance is not used for it.
    class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    class.hbrBackground = BACKGROUND_BRUSH as HBRUSH;
    class.lpszClassName = class_name.as_ptr();

    // SAFETY: all pointers in class are valid and live until the end of the function.
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        show_error("failed to register the window class");
        return 2;
    }

    let dpi = win32::system_dpi();
    // SAFETY: the strings live until the end of the expression, instance comes from the system.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            model::scale(980, dpi),
            model::scale(640, dpi),
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if hwnd.is_null() {
        show_error("failed to create the main window");
        return 3;
    }

    // The window owns the state; it is freed in WM_DESTROY.
    let state = Box::into_raw(State::new());
    // SAFETY: hwnd is alive; the Box pointer is not freed before WM_DESTROY.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
    }
    apply_settings(hwnd, &load_settings());
    refresh(hwnd);

    win32::show(hwnd);
    // SAFETY: hwnd is created and not destroyed yet.
    unsafe {
        UpdateWindow(hwnd);
    }

    message_loop()
}

/// Message loop until `WM_QUIT`.
fn message_loop() -> u8 {
    // SAFETY: MSG is a plain Win32 struct; zeros are a valid initial value.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: the message pointer is valid, the filter means all messages.
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        match result {
            0 => return message.wParam as u8,
            -1 => return 1,
            _ => {}
        }
        // SAFETY: message is filled by GetMessageW.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Pointer to the window state (NULL before `run()` and after `WM_DESTROY`).
fn state_ptr(hwnd: HWND) -> *mut State {
    // SAFETY: GWLP_USERDATA is set by us in run(); the value is a Box pointer.
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State }
}

/// Run an action on the window state when it already exists.
fn with_state<R>(hwnd: HWND, action: impl FnOnce(&mut State) -> R) -> Option<R> {
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the pointer is valid while the window is alive; calls come from the UI thread.
    let state = unsafe { &mut *pointer };
    Some(action(state))
}

/// Take the state when the window is destroyed.
fn take_state(hwnd: HWND) {
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return;
    }
    // SAFETY: the pointer came from Box::into_raw in run() and is not used any more.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(pointer));
    }
}

/// Window procedure: controls, layout, commands, drag and drop.
///
/// # Safety
///
/// Called by the system only; `hwnd` is a live window of our class.
unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            win32::accept_dropped_files(hwnd);
            create_controls(hwnd);
            layout_controls(hwnd);
            0
        }
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            // Do not let the window shrink below the minimum layout size.
            let info = lparam as *mut MINMAXINFO;
            if !info.is_null() {
                let dpi = win32::dpi_of(hwnd);
                // SAFETY: Win32 passes a valid MINMAXINFO pointer in this message.
                let info = unsafe { &mut *info };
                info.ptMinTrackSize.x =
                    model::scale(model::MIN_SIZE_96.0, dpi) + model::scale(32, dpi);
                info.ptMinTrackSize.y =
                    model::scale(model::MIN_SIZE_96.1, dpi) + model::scale(96, dpi);
            }
            0
        }
        WM_DPICHANGED => {
            // lparam is the suggested window rectangle for the new monitor.
            let suggested = lparam as *const RECT;
            if !suggested.is_null() {
                // SAFETY: Win32 passes a valid RECT pointer in this message.
                let rect = unsafe { *suggested };
                // SAFETY: hwnd is alive; coordinates come from the suggested rectangle.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            layout_controls(hwnd);
            0
        }
        WM_COMMAND => {
            handle_command(hwnd, (wparam & 0xFFFF) as usize);
            0
        }
        WM_TIMER => {
            if wparam == TIMER_ID {
                on_timer(hwnd);
            }
            0
        }
        WM_DROPFILES => {
            handle_drop(hwnd, lparam as *mut c_void);
            0
        }
        WM_CLOSE => {
            save_settings(hwnd);
            win32::destroy(hwnd);
            0
        }
        WM_DESTROY => {
            // Cancel the background work and kill the timer before the state is freed.
            with_state(hwnd, |state| {
                if let Some(job) = state.job.take() {
                    job.shared.cancel();
                    let _ = job.handle.join();
                }
            });
            // SAFETY: hwnd is still alive in this message; the timer belongs to the window.
            unsafe {
                KillTimer(hwnd, TIMER_ID);
            }
            take_state(hwnd);
            // SAFETY: WM_DESTROY arrives last for this thread.
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => {
            // SAFETY: DefWindowProcW is the default handler for the remaining messages.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

/// Create the window controls.
fn create_controls(hwnd: HWND) {
    let version = model::version_line();
    win32::create_child(hwnd, "STATIC", &version, WS_CHILD | WS_VISIBLE, Id::Title.raw());
    win32::create_child(hwnd, "STATIC", model::HINT_EMPTY, WS_CHILD | WS_VISIBLE, Id::Hint.raw());

    let list_style =
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS;
    let list = win32::create_child_ex(
        hwnd,
        WS_EX_CLIENTEDGE,
        "SysListView32",
        "",
        list_style,
        Id::List.raw(),
    );
    win32::list_init(list, win32::dpi_of(hwnd));

    // Conversion parameters: labels, fields, combo box, check boxes.
    for (id, text) in [
        (Id::LabelOutDir, "Output folder"),
        (Id::LabelUnits, "Scale"),
        (Id::LabelRecenter, "Recenter"),
        (Id::LabelMaxTile, "Points per tile"),
        (Id::LabelGuid, "GUID"),
    ] {
        win32::create_child(hwnd, "STATIC", text, WS_CHILD | WS_VISIBLE, id.raw());
    }
    for id in [Id::OutDir, Id::Units, Id::MaxTile, Id::Guid] {
        win32::create_child_ex(
            hwnd,
            WS_EX_CLIENTEDGE,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            id.raw(),
        );
    }
    let combo = win32::create_child_ex(
        hwnd,
        0,
        "COMBOBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        Id::Recenter.raw(),
    );
    let labels: Vec<&str> = RecenterChoice::ALL.iter().map(|choice| choice.label()).collect();
    win32::combo_fill(combo, &labels, 0);

    for (id, text) in [
        (Id::ColorCheck, "Colour"),
        (Id::IntensityCheck, "Intensity"),
        (Id::ForceCheck, "Overwrite .rcs"),
        (Id::RecursiveCheck, "Include subfolders"),
    ] {
        win32::create_child(
            hwnd,
            "BUTTON",
            text,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            id.raw(),
        );
    }
    win32::create_child(
        hwnd,
        "BUTTON",
        "Browse...",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        Id::BrowseOut.raw(),
    );

    // Progress bar and action buttons.
    let bar = win32::create_child_ex(
        hwnd,
        WS_EX_CLIENTEDGE,
        "msctls_progress32",
        "",
        WS_CHILD | WS_VISIBLE | PBS_SMOOTH as u32,
        Id::Progress.raw(),
    );
    win32::progress_range(bar, 100);
    win32::create_child(
        hwnd,
        "BUTTON",
        "Convert",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        Id::Convert.raw(),
    );
    win32::create_child(
        hwnd,
        "BUTTON",
        "Open folder",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        Id::OpenOut.raw(),
    );

    for (id, text) in [
        (Id::AddFiles, "Add files..."),
        (Id::AddFolder, "Add folder..."),
        (Id::Remove, "Remove"),
        (Id::Clear, "Clear"),
        (Id::Close, "Close"),
    ] {
        win32::create_child(
            hwnd,
            "BUTTON",
            text,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            id.raw(),
        );
    }
}

/// Lay the controls out for the current client size and dpi.
fn layout_controls(hwnd: HWND) {
    let (width, height) = win32::client_size(hwnd);
    let dpi = win32::dpi_of(hwnd);
    let layout = model::layout(width, height, dpi);
    for (id, rect) in [
        (Id::Title, layout.title),
        (Id::Hint, layout.hint),
        (Id::List, layout.list),
        (Id::LabelOutDir, layout.label_out_dir),
        (Id::OutDir, layout.out_dir),
        (Id::BrowseOut, layout.browse_out),
        (Id::LabelUnits, layout.label_units),
        (Id::Units, layout.units),
        (Id::LabelRecenter, layout.label_recenter),
        (Id::Recenter, layout.recenter),
        (Id::LabelMaxTile, layout.label_max_tile),
        (Id::MaxTile, layout.max_tile),
        (Id::LabelGuid, layout.label_guid),
        (Id::Guid, layout.guid),
        (Id::ColorCheck, layout.color),
        (Id::IntensityCheck, layout.intensity),
        (Id::ForceCheck, layout.force),
        (Id::RecursiveCheck, layout.recursive),
        (Id::Convert, layout.convert),
        (Id::OpenOut, layout.open_out),
        (Id::Progress, layout.progress),
        (Id::AddFiles, layout.add_files),
        (Id::AddFolder, layout.add_folder),
        (Id::Remove, layout.remove),
        (Id::Clear, layout.clear),
        (Id::Close, layout.close),
    ] {
        win32::move_child(hwnd, id.raw(), rect);
    }
}

/// Handle a button press or a parameter change.
fn handle_command(hwnd: HWND, id: usize) {
    if id == Id::AddFiles.raw() {
        add_files(hwnd);
    } else if id == Id::AddFolder.raw() {
        add_folder(hwnd);
    } else if id == Id::Remove.raw() {
        remove_selected(hwnd);
    } else if id == Id::Clear.raw() {
        let _ = with_state(hwnd, |state| {
            state.queue.clear();
            state.summary = None;
        });
        refresh(hwnd);
    } else if id == Id::BrowseOut.raw() {
        browse_output(hwnd);
    } else if id == Id::Convert.raw() {
        toggle_convert(hwnd);
    } else if id == Id::OpenOut.raw() {
        open_output_folder(hwnd);
    } else if id == Id::Close.raw() {
        save_settings(hwnd);
        win32::destroy(hwnd);
    } else if is_parameter(id) {
        // A parameter changed: refresh the hint (which also shows input errors).
        refresh(hwnd);
    }
}

/// Whether `id` belongs to the parameter fields.
fn is_parameter(id: usize) -> bool {
    [
        Id::OutDir,
        Id::Units,
        Id::Recenter,
        Id::MaxTile,
        Id::Guid,
        Id::ColorCheck,
        Id::IntensityCheck,
        Id::ForceCheck,
        Id::RecursiveCheck,
    ]
    .iter()
    .any(|candidate| candidate.raw() == id)
}

/// "Browse..." — choose the output folder.
fn browse_output(hwnd: HWND) {
    let Some(folder) = win32::browse_folder(hwnd, "Folder for the results") else {
        return;
    };
    let edit = win32::child(hwnd, Id::OutDir.raw());
    win32::set_text(edit, &folder.display().to_string());
    refresh(hwnd);
}

/// "Convert" / "Cancel" — one button with two states.
fn toggle_convert(hwnd: HWND) {
    let running = with_state(hwnd, |state| state.job.is_some()).unwrap_or(false);
    if running {
        cancel_convert(hwnd);
    } else {
        start_convert(hwnd);
    }
}

/// Start converting the queue in a background thread.
fn start_convert(hwnd: HWND) {
    let settings = read_settings(hwnd);
    if let Err(message) = settings.validate() {
        warn(hwnd, &format!("Check the settings: {message}"));
        return;
    }

    let (paths, empty) = with_state(hwnd, |state| {
        state.queue.reset_statuses();
        state.applied = 0;
        state.summary = None;
        (state.queue.paths(), state.queue.is_empty())
    })
    .unwrap_or((Vec::new(), true));
    if empty {
        warn(hwnd, "The queue is empty: add .las/.laz/.pts files.");
        return;
    }

    let shared = Shared::new();
    let handle = job::spawn(paths, settings, shared.clone());
    let _ = with_state(hwnd, |state| state.job = Some(Job { shared, handle }));

    refresh(hwnd);
    let bar = win32::child(hwnd, Id::Progress.raw());
    win32::progress_set(bar, 0);
    win32::set_text(win32::child(hwnd, Id::Convert.raw()), "Cancel");
    // SAFETY: hwnd is alive; the timer is killed in on_timer/WM_DESTROY.
    unsafe {
        SetTimer(hwnd, TIMER_ID, TIMER_PERIOD, None);
    }
}

/// Ask the background thread to stop.
fn cancel_convert(hwnd: HWND) {
    let _ = with_state(hwnd, |state| {
        if let Some(job) = &state.job {
            job.shared.cancel();
        }
    });
    win32::set_text(win32::child(hwnd, Id::Convert.raw()), "Cancelling...");
}

/// Poll the background thread: progress bar, row states, final summary.
fn on_timer(hwnd: HWND) {
    let info = with_state(hwnd, |state| {
        let job = state.job.as_ref()?;
        Some((job.shared.progress(), job.shared.results(), job.shared.is_finished()))
    })
    .flatten();
    let Some(((stage, done, total), results, finished)) = info else {
        return;
    };

    let bar = win32::child(hwnd, Id::Progress.raw());
    let percent = if total > 0 { ((done * 100) / total).min(100) as u32 } else { 0 };
    win32::progress_set(bar, percent);
    let hint = win32::child(hwnd, Id::Hint.raw());
    win32::set_text(
        hint,
        &format!("{}: {} of {} ({} %)", stage.title(), done, total, percent),
    );

    let _ = with_state(hwnd, |state| {
        let list = win32::child(hwnd, Id::List.raw());
        while state.applied < results.len() {
            let index = state.applied;
            let status = entry_status(results[index].status);
            state.queue.set_status(index, status);
            win32::list_set_cell(list, index, 3, status.label());
            state.applied += 1;
        }
    });

    if !finished {
        return;
    }

    // SAFETY: hwnd is alive; the timer is no longer needed.
    unsafe {
        KillTimer(hwnd, TIMER_ID);
    }
    let summary = with_state(hwnd, |state| {
        if let Some(job) = state.job.take() {
            let _ = job.handle.join();
        }
        state.applied = results.len();
        let mut written = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut cancelled = 0;
        for result in &results {
            match result.status {
                rcs_conv::Status::Written => written += 1,
                rcs_conv::Status::Skipped => skipped += 1,
                rcs_conv::Status::Failed => failed += 1,
                rcs_conv::Status::Cancelled => cancelled += 1,
            }
        }
        let mut text =
            format!("Done: {written} written, {skipped} skipped, {failed} failed");
        if cancelled > 0 {
            text.push_str(&format!(", {cancelled} cancelled"));
        }
        state.summary = Some(text.clone());
        text
    })
    .unwrap_or_else(|| "Done".to_string());

    win32::set_text(win32::child(hwnd, Id::Convert.raw()), "Convert");
    win32::progress_set(bar, 100);
    refresh(hwnd);
    win32::set_text(win32::child(hwnd, Id::Hint.raw()), &summary);
}

/// Result of one file conversion -> list row state.
fn entry_status(status: rcs_conv::Status) -> EntryStatus {
    match status {
        rcs_conv::Status::Written => EntryStatus::Written,
        rcs_conv::Status::Skipped => EntryStatus::Skipped,
        rcs_conv::Status::Failed => EntryStatus::Failed,
        rcs_conv::Status::Cancelled => EntryStatus::Cancelled,
    }
}

/// "Open folder" — show the result directory in Explorer.
fn open_output_folder(hwnd: HWND) {
    let settings = read_settings(hwnd);
    let folder = if settings.out_dir.trim().is_empty() {
        let first = with_state(hwnd, |state| {
            state.queue.entries().first().map(|entry| entry.path.clone())
        })
        .flatten();
        match first.and_then(|path| path.parent().map(|parent| parent.to_path_buf())) {
            Some(parent) => parent,
            None => {
                warn(hwnd, "Nothing to open: set an output folder or add files.");
                return;
            }
        }
    } else {
        std::path::PathBuf::from(settings.out_dir.trim())
    };
    if !folder.exists() {
        warn(hwnd, &format!("The folder does not exist yet: {}", folder.display()));
        return;
    }
    if !win32::shell_open(&folder) {
        warn(hwnd, &format!("Cannot open {}", folder.display()));
    }
}

/// Short error message on top of the window.
fn warn(hwnd: HWND, text: &str) {
    let text = win32::wide(text);
    let caption = win32::wide("rcsconv-gui");
    // SAFETY: the strings live until MessageBoxW returns; hwnd owns the window.
    unsafe {
        MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), MB_ICONWARNING | MB_OK);
    }
}

/// "Add files..." — multi-select dialog.
fn add_files(hwnd: HWND) {
    let files = win32::open_files(hwnd, "Add point clouds", &OPEN_FILTER);
    if files.is_empty() {
        return;
    }
    let _ = with_state(hwnd, |state| {
        state.queue.add(&files, false);
        state.summary = None;
    });
    refresh(hwnd);
}

/// "Add folder..." — a directory with all files (walking subdirectories).
fn add_folder(hwnd: HWND) {
    let Some(folder) = win32::browse_folder(hwnd, "Select a folder with point clouds") else {
        return;
    };
    let _ = with_state(hwnd, |state| {
        state.queue.add(&[folder], true);
        state.summary = None;
    });
    refresh(hwnd);
}

/// "Remove" — delete the selected file from the queue.
fn remove_selected(hwnd: HWND) {
    let list = win32::child(hwnd, Id::List.raw());
    let Some(index) = win32::list_selected(list) else {
        return;
    };
    let _ = with_state(hwnd, |state| {
        state.queue.remove(index);
        state.summary = None;
    });
    refresh(hwnd);
}

/// Dragging files into the window.
fn handle_drop(hwnd: HWND, hdrop: *mut c_void) {
    let files = win32::dropped_files(hdrop);
    if files.is_empty() {
        return;
    }
    let _ = with_state(hwnd, |state| {
        state.queue.add(&files, true);
        state.summary = None;
    });
    refresh(hwnd);
}

/// Redraw the file list and the queue hint.
fn refresh(hwnd: HWND) {
    let list = win32::child(hwnd, Id::List.raw());
    win32::list_clear(list);
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return;
    }
    // SAFETY: the pointer is valid while the window is alive; we only read the queue state.
    let state = unsafe { &*pointer };
    for (index, entry) in state.queue.entries().iter().enumerate() {
        let name = entry
            .path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = entry
            .path
            .parent()
            .map(|value| value.display().to_string())
            .unwrap_or_default();
        let cells = vec![
            name,
            folder,
            model::format_size(entry.size),
            entry.status.label().to_string(),
        ];
        win32::list_add_row(list, index, &cells);
    }
    let hint = win32::child(hwnd, Id::Hint.raw());
    win32::set_text(hint, &hint_text(hwnd, state));
}

/// Registry value names.
const SETTINGS_NAMES: &[&str] = &[
    "out_dir",
    "units",
    "recenter",
    "max_per_tile",
    "guid",
    "color",
    "intensity",
    "force",
    "recursive",
];

/// Read the parameters from the window fields.
fn read_settings(hwnd: HWND) -> Settings {
    let text = |id: Id| win32::text_of(win32::child(hwnd, id.raw()));
    let check = |id: Id| win32::check_get(win32::child(hwnd, id.raw()));
    let recenter = win32::combo_selected(win32::child(hwnd, Id::Recenter.raw()))
        .and_then(|index| RecenterChoice::ALL.get(index).copied())
        .unwrap_or(RecenterChoice::None);
    Settings {
        out_dir: text(Id::OutDir),
        units: text(Id::Units),
        recenter,
        max_per_tile: text(Id::MaxTile),
        guid: text(Id::Guid),
        color: check(Id::ColorCheck),
        intensity: check(Id::IntensityCheck),
        force: check(Id::ForceCheck),
        recursive: check(Id::RecursiveCheck),
    }
}

/// Show the parameters in the window fields.
fn apply_settings(hwnd: HWND, settings: &Settings) {
    win32::set_text(win32::child(hwnd, Id::OutDir.raw()), &settings.out_dir);
    win32::set_text(win32::child(hwnd, Id::Units.raw()), &settings.units);
    win32::set_text(win32::child(hwnd, Id::MaxTile.raw()), &settings.max_per_tile);
    win32::set_text(win32::child(hwnd, Id::Guid.raw()), &settings.guid);

    let recenter = RecenterChoice::ALL
        .iter()
        .position(|choice| *choice == settings.recenter)
        .unwrap_or(0);
    win32::combo_select(win32::child(hwnd, Id::Recenter.raw()), recenter);

    win32::check_set(win32::child(hwnd, Id::ColorCheck.raw()), settings.color);
    win32::check_set(win32::child(hwnd, Id::IntensityCheck.raw()), settings.intensity);
    win32::check_set(win32::child(hwnd, Id::ForceCheck.raw()), settings.force);
    win32::check_set(win32::child(hwnd, Id::RecursiveCheck.raw()), settings.recursive);
}

/// Read the settings from `HKCU\Software\rcsconv\gui`.
fn load_settings() -> Settings {
    let values = win32::registry_load(SETTINGS_NAMES);
    Settings::from_pairs(values.iter().map(|(name, value)| (name.as_str(), value.as_str())))
}

/// Save the settings to `HKCU\Software\rcsconv\gui`.
fn save_settings(hwnd: HWND) {
    let settings = read_settings(hwnd);
    win32::registry_save(&settings.to_pairs());
}

/// Hint: run summary, parameter errors or queue state.
fn hint_text(hwnd: HWND, state: &State) -> String {
    if let Some(summary) = &state.summary {
        return summary.clone();
    }
    match read_settings(hwnd).validate() {
        Ok(()) => model::queue_hint(&state.queue),
        Err(message) => format!("Settings error: {message}"),
    }
}

/// Show an error in a modal box (used before the main window appears).
fn show_error(text: &str) {
    let text = win32::wide(text);
    let caption = win32::wide("rcsconv-gui");
    // SAFETY: both strings live until MessageBoxW returns.
    unsafe {
        MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_ICONERROR | MB_OK);
    }
}
