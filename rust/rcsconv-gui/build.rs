//! Вкомпилировать манифест и VERSIONINFO в `rcsconv-gui.exe`.
//!
//! Работает только для GNU-таргета Windows: `windres` берётся из `PATH` (MinGW,
//! на машине владельца — `C:\mingw64\bin\windres.exe`). На других платформах
//! шаг пропускается, поэтому `cargo check` на Linux проходит без ресурсов.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-env-changed=WINDRES");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        // MSVC в проекте не используется (.clinerules п.10), ресурсы ему не нужны.
        println!("cargo:warning=ресурсы вкомпилируются только для GNU-таргета Windows");
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let object = out_dir.join("app_res.o");
    let windres = env::var("WINDRES").unwrap_or_else(|_| "windres".to_string());

    let status = Command::new(&windres)
        .args(["--input", "res/app.rc", "--output-format=coff", "--include-dir=res"])
        .arg(&object)
        .status();

    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(status) => panic!("windres завершился с кодом {status}: манифест не вкомпилирован"),
        Err(err) => println!(
            "cargo:warning=windres не найден ({err}): GUI соберётся без манифеста \
             (без визуальных стилей comctl32 v6 и без DPI-настроек)"
        ),
    }
}
