//! CLI integration tests: run `rcsconv` as a process.
//!
//! Checks the result invariants (`docs/RCS-FORMAT.md` 10) and the interface
//! behaviour. Byte parity with Python on the reference files is checked
//! separately (the build itself — `cargo test` plus `out/py` in a manual run, see
//! `PLAN.md`).
//!
//! The data (`golden/`) is not in git: without it the tests print "skipped" and pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use rcs_format::metadata::Metadata;
use rcs_format::{point, tile, Container};

fn golden(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden").join(name);
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipped: no {}", path.display());
        None
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rcsconv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temporary directory");
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rcsconv"))
        .args(args)
        .output()
        .expect("start rcsconv")
}

/// Check the invariants of a built file: blocks, tiles, metadata, points.
fn assert_cloud_file(path: &Path, expected_points: u64, expected_tiles: usize, words: &[u32]) {
    let data = std::fs::read(path).expect("read result");
    let container = Container::parse(&data).expect("parse result");
    assert_eq!(container.directory_offset(), 0x135, "directory at 0x135");
    assert_eq!(container.block_types(), vec![3, 1, 11, 0], "block order");
    assert_eq!(container.header().count, expected_points, "header count");
    assert_eq!(container.header().chunk_version, 5, "Unified RCS export");

    let tiles = tile::iter_tiles(&container).expect("tiles");
    assert_eq!(tiles.len(), expected_tiles, "number of tiles");
    let sum: u64 = tiles.iter().map(|tile| tile.count as u64).sum();
    assert_eq!(sum, expected_points, "sum of tile counts");

    let block = container.block_of_type(1).expect("type 1 block");
    let meta = Metadata::parse(container.read_block(block)).expect("type 1 parse");
    assert_eq!(meta.levels(), words, "occupied tiles per level");
    assert!(meta.is_consistent(), "K = sum(words)");

    let points = point::decode_all(&container).expect("points");
    assert_eq!(points.len() as u64, expected_points, "all points decode");
}

#[test]
fn version_and_help() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("rcsconv "), "version: {text}");

    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("las2rcs") && text.contains("pts2rcs"));
}

#[test]
fn unknown_flag_is_rejected() {
    let output = run(&["las2rcs", "a.laz", "--wobble"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn missing_input_is_reported() {
    let output = run(&["pts2rcs", "no-such-file.pts"]);
    assert_eq!(output.status.code(), Some(1), "input read error");
}

#[test]
fn pts2rcs_builds_level_7() {
    let Some(source) = golden("level_7.pts") else {
        return;
    };
    let out = temp_dir("pts");
    let output = run(&[
        "pts2rcs",
        source.to_str().expect("path"),
        "-o",
        out.to_str().expect("path"),
    ]);
    assert!(output.status.success(), "exit code 0: {output:?}");
    let target = out.join("level_7.rcs");
    assert!(target.exists(), "file created");
    assert_cloud_file(&target, 324_680, 43, &[1, 8, 13, 43]);
    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn las2rcs_builds_level_7() {
    let Some(source) = golden("level_7.laz") else {
        return;
    };
    let out = temp_dir("las");
    let output = run(&[
        "las2rcs",
        source.to_str().expect("path"),
        "-o",
        out.to_str().expect("path"),
    ]);
    assert!(output.status.success(), "exit code 0: {output:?}");
    let target = out.join("level_7.rcs");
    assert_cloud_file(&target, 324_680, 43, &[1, 8, 13, 43]);
    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn existing_output_is_overwritten_by_default() {
    let Some(source) = golden("probe6.pts") else {
        return;
    };
    let out = temp_dir("overwrite");
    let source_text = source.to_str().expect("path");
    let out_text = out.to_str().expect("path");
    assert!(run(&["pts2rcs", source_text, "-o", out_text]).status.success(), "first run");
    let target = out.join("probe6.rcs");
    let first = std::fs::read(&target).expect("first file");

    let output = run(&[
        "pts2rcs",
        source_text,
        "-o",
        out_text,
        "-g",
        "{11112233_4455_6677_8899_aabbccddeeff}",
    ]);
    assert!(output.status.success(), "second run: {output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("skipped"), "overwrite by default: {text}");
    let second = std::fs::read(&target).expect("second file");
    assert_ne!(first, second, "the file must be rewritten with the new GUID");
    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn no_overwrite_flag_skips_existing() {
    let Some(source) = golden("probe6.pts") else {
        return;
    };
    let out = temp_dir("no-overwrite");
    let source_text = source.to_str().expect("path");
    let out_text = out.to_str().expect("path");
    assert!(run(&["pts2rcs", source_text, "-o", out_text]).status.success(), "first run");
    let target = out.join("probe6.rcs");
    let first = std::fs::read(&target).expect("first file");

    let output = run(&[
        "pts2rcs",
        source_text,
        "-o",
        out_text,
        "--no-overwrite",
        "-g",
        "{11112233_4455_6677_8899_aabbccddeeff}",
    ]);
    assert!(output.status.success(), "skip run: {output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("skipped"), "skip message: {text}");
    let second = std::fs::read(&target).expect("second file");
    assert_eq!(first, second, "--no-overwrite must not touch the file");
    let _ = std::fs::remove_dir_all(out);
}
