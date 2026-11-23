//! `rcsconv-gui.exe` — a Win32 interface (no framework).
//!
//! A separate binary next to the console `rcsconv.exe`: the CLI keeps its console
//! and exit codes, the GUI uses the `windows` subsystem (no console window).
//! Both link the same crates (`rcs-conv` and below), so the conversion result is
//! identical (checked in G4).
//!
//! `unsafe` is allowed only in the [`win32`] module and the message handlers
//! (`.clinerules` 10); the other modules are pure Rust.

#![cfg_attr(windows, windows_subsystem = "windows")]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod app;
mod job;
mod model;
mod params;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(app::run())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rcsconv-gui: the interface is Windows-only (Win32)");
}
