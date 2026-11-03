//! Интеграционные тесты CLI: запуск `rcsconv` как процесса.
//!
//! Проверяются инварианты результата (`docs/RCS-FORMAT.md` §10) и поведение
//! интерфейса. Побайтовый паритет с Python на эталонах проверяется отдельно
//! (`сама сборка` — `cargo test` + `out/py` в ручном прогоне, см. `PLAN.md`).
//!
//! Данные (`golden/`) в git не входят: тесты без них печатают «пропуск» и проходят.

use std::path::{Path, PathBuf};
use std::process::Command;

use rcs_format::metadata::Metadata;
use rcs_format::{point, tile, Container};

fn golden(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden").join(name);
    if path.exists() {
        Some(path)
    } else {
        eprintln!("пропуск: нет {}", path.display());
        None
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rcsconv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("временный каталог");
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rcsconv"))
        .args(args)
        .output()
        .expect("запуск rcsconv")
}

/// Проверить инварианты собранного файла: блоки, узлы, метаданные, точки.
fn assert_cloud_file(path: &Path, expected_points: u64, expected_tiles: usize, words: &[u32]) {
    let data = std::fs::read(path).expect("чтение результата");
    let container = Container::parse(&data).expect("разбор результата");
    assert_eq!(container.directory_offset(), 0x135, "директория на 0x135");
    assert_eq!(container.block_types(), vec![3, 1, 11, 0], "порядок блоков");
    assert_eq!(container.header().count, expected_points, "count заголовка");
    assert_eq!(container.header().chunk_version, 5, "экспорт Unified RCS");

    let tiles = tile::iter_tiles(&container).expect("узлы");
    assert_eq!(tiles.len(), expected_tiles, "число узлов");
    let sum: u64 = tiles.iter().map(|tile| tile.count as u64).sum();
    assert_eq!(sum, expected_points, "сумма count по узлам");

    let block = container.block_of_type(1).expect("блок type 1");
    let meta = Metadata::parse(container.read_block(block)).expect("разбор type 1");
    assert_eq!(meta.levels(), words, "занятые узлы по уровням");
    assert!(meta.is_consistent(), "K = sum(words)");

    let points = point::decode_all(&container).expect("точки");
    assert_eq!(points.len() as u64, expected_points, "точки декодируются все");
}

#[test]
fn version_and_help() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("rcsconv "), "версия: {text}");

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
    assert_eq!(output.status.code(), Some(1), "ошибка чтения входа");
}

#[test]
fn pts2rcs_builds_level_7() {
    let Some(source) = golden("level_7.pts") else {
        return;
    };
    let out = temp_dir("pts");
    let output = run(&[
        "pts2rcs",
        source.to_str().expect("путь"),
        "-o",
        out.to_str().expect("путь"),
    ]);
    assert!(output.status.success(), "код 0: {output:?}");
    let target = out.join("level_7.rcs");
    assert!(target.exists(), "файл создан");
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
        source.to_str().expect("путь"),
        "-o",
        out.to_str().expect("путь"),
    ]);
    assert!(output.status.success(), "код 0: {output:?}");
    let target = out.join("level_7.rcs");
    assert_cloud_file(&target, 324_680, 43, &[1, 8, 13, 43]);
    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn existing_output_requires_force() {
    let Some(source) = golden("probe6.pts") else {
        return;
    };
    let out = temp_dir("force");
    let args = [
        "pts2rcs",
        source.to_str().expect("путь"),
        "-o",
        out.to_str().expect("путь"),
    ];
    assert!(run(&args).status.success(), "первый прогон");
    let output = run(&args);
    assert!(output.status.success(), "второй прогон без -f");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("пропущено"), "сообщение о пропуске: {text}");
    let _ = std::fs::remove_dir_all(out);
}
