//! `rcsconv-gui.exe` — интерфейс на Win32 (без фреймворка).
//!
//! Отдельный бинарник рядом с консольным `rcsconv.exe`: CLI остаётся с консолью
//! и прежними кодами выхода, GUI — подсистема `windows` (без окна консоли).
//! Оба линкуют одни и те же крейты (`rcs-conv` и ниже), поэтому результат
//! конвертации один и тот же (проверяется в G4).
//!
//! `unsafe` разрешён только в модуле [`win32`] и обработчиках сообщений
//! (`.clinerules` п.10); остальные модули — чистый Rust.

#![cfg_attr(windows, windows_subsystem = "windows")]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod app;
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
    eprintln!("rcsconv-gui: интерфейс только для Windows (Win32)");
}
