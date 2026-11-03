//! CLI-слой: разбор аргументов, разворачивание входов, цикл по файлам.
//!
//! Правило интерфейса повторяет Python-эталон (`tools/rcs_cli.py`, README):
//! первый аргумент — вход (файл, каталог или маска), имя выхода подставляется
//! само (`<имя>.rcs`), существующий файл не перезаписывается без `-f`.

use std::path::{Path, PathBuf};

use rcs_cloud::{build_from_source, BuildOptions};

/// Общие параметры конвертеров.
pub struct Options {
    /// Входные файлы, каталоги и маски.
    pub sources: Vec<String>,
    /// Куда писать (по умолчанию рядом с входом).
    pub out_dir: Option<PathBuf>,
    /// Предел точек в узле.
    pub max_per_tile: u32,
    /// Перезаписывать существующий `.rcs`.
    pub force: bool,
    /// Обойти подкаталоги.
    pub recursive: bool,
    /// GUID результата (по умолчанию случайный).
    pub guid: Option<String>,
    /// Настройки чтения LAS/LAZ.
    pub las: cloud_las::LasOptions,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sources: Vec::new(),
            out_dir: None,
            max_per_tile: rcs_cloud::MAX_TILE_POINTS,
            force: false,
            recursive: false,
            guid: None,
            las: cloud_las::LasOptions::default(),
        }
    }
}

/// Ошибка разбора аргументов.
pub struct ArgsError(pub String);

impl Options {
    /// Разобрать аргументы командной строки.
    ///
    /// `las` — разрешены ли ключи LAS/LAZ (`--units-scale`, `--recenter`, …).
    pub fn parse(args: &[String], las: bool) -> Result<Options, ArgsError> {
        let mut options = Options::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].clone();
            let mut take = |name: &str| -> Result<String, ArgsError> {
                index += 1;
                args.get(index)
                    .cloned()
                    .ok_or_else(|| ArgsError(format!("{name}: нужно значение")))
            };
            match arg.as_str() {
                "-o" | "--out-dir" => options.out_dir = Some(PathBuf::from(take(&arg)?)),
                "-m" | "--max-points-per-tile" => {
                    let text = take(&arg)?;
                    options.max_per_tile = text
                        .parse()
                        .map_err(|_| ArgsError(format!("-m {text}: ожидается целое число")))?;
                }
                "-f" | "--force" => options.force = true,
                "-r" | "--recursive" => options.recursive = true,
                "-g" | "--guid" => options.guid = Some(take(&arg)?),
                "--units-scale" if las => {
                    let text = take(&arg)?;
                    options.las.units = text
                        .parse()
                        .map_err(|_| ArgsError(format!("--units-scale {text}: ожидается число")))?;
                    if options.las.units <= 0.0 {
                        return Err(ArgsError("--units-scale должен быть > 0".into()));
                    }
                }
                "--recenter" if las => {
                    let text = take(&arg)?;
                    options.las.recenter =
                        cloud_las::Recenter::parse(&text).ok_or_else(|| {
                            ArgsError(format!("--recenter {text}: допустимо none|center|min"))
                        })?;
                }
                "--no-color" if las => options.las.color = false,
                "--no-intensity" if las => options.las.intensity = false,
                "--color-scale" if las => {
                    let text = take(&arg)?;
                    options.las.color_scale = cloud_las::ScaleMode::parse(&text).ok_or_else(
                        || ArgsError(format!("--color-scale {text}: допустимо auto|257|1")),
                    )?;
                }
                "--intensity-scale" if las => {
                    let text = take(&arg)?;
                    options.las.intensity_scale =
                        cloud_las::ScaleMode::parse(&text).ok_or_else(|| {
                            ArgsError(format!("--intensity-scale {text}: допустимо auto|257|1"))
                        })?;
                }
                other if other.starts_with('-') => {
                    return Err(ArgsError(format!("неизвестный аргумент: {other}")));
                }
                other => options.sources.push(other.to_string()),
            }
            index += 1;
        }
        Ok(options)
    }
}

/// Один результат конвертации.
enum Outcome {
    Written,
    Skipped,
    Failed,
}

/// Пройти по входам и собрать `.rcs`; вернуть код выхода.
pub fn run(command: &str, args: &[String]) -> i32 {
    let las = command == "las2rcs";
    let options = match Options::parse(args, las) {
        Ok(options) => options,
        Err(ArgsError(message)) => {
            eprintln!("rcsconv {command}: {message}");
            return 2;
        }
    };
    if options.sources.is_empty() {
        eprintln!("rcsconv {command}: не указаны входные файлы");
        return 2;
    }
    if options.max_per_tile == 0 || options.max_per_tile > rcs_cloud::MAX_TILE_POINTS {
        eprintln!(
            "rcsconv {command}: -m {} вне 1…{}",
            options.max_per_tile,
            rcs_cloud::MAX_TILE_POINTS
        );
        return 2;
    }
    let guid = match &options.guid {
        Some(text) => match rcs_cloud::normalize_guid(text) {
            Ok(value) => Some(value),
            Err(err) => {
                eprintln!("rcsconv {command}: {err}");
                return 2;
            }
        },
        None => None,
    };
    let build_options = BuildOptions { max_per_tile: options.max_per_tile, guid };

    let suffixes: &[&str] = if las { &[".las", ".laz"] } else { &[".pts"] };
    let sources = expand_sources(&options.sources, suffixes, options.recursive);
    if sources.is_empty() {
        eprintln!("rcsconv {command}: входные файлы не найдены");
        return 2;
    }
    if let Some(wrong) = sources.iter().find(|path| has_suffix(path, ".rcs")) {
        eprintln!(
            "rcsconv {command}: {} — это .rcs, а конвертер читает {}",
            wrong.display(),
            if las { ".las/.laz" } else { ".pts" }
        );
        return 2;
    }

    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for source in &sources {
        match convert_one(source, &options, &build_options, las) {
            Outcome::Written => written += 1,
            Outcome::Skipped => skipped += 1,
            Outcome::Failed => failed += 1,
        }
    }
    if sources.len() > 1 {
        println!("итого: записано {written}, пропущено {skipped}, ошибок {failed}");
    }
    if failed == 0 {
        0
    } else {
        1
    }
}


/// Сконвертировать один файл.
fn convert_one(
    source: &Path,
    options: &Options,
    build_options: &BuildOptions,
    las: bool,
) -> Outcome {
    let target = default_target(source, options.out_dir.as_deref());
    if target.exists() && !options.force {
        println!(
            "{} → {}: пропущено (файл есть, нужен -f)",
            source.display(),
            target.display()
        );
        return Outcome::Skipped;
    }

    let built = if las {
        let mut reader = cloud_las::LasSource::new(source, options.las.clone());
        build_from_source(&mut reader, build_options)
    } else {
        let mut reader = cloud_pts::PtsSource::new(source);
        build_from_source(&mut reader, build_options)
    };
    let (blob, stats) = match built {
        Ok(built) => built,
        Err(err) => {
            eprintln!("{}: ОШИБКА: {err}", source.display());
            return Outcome::Failed;
        }
    };

    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                eprintln!("{}: ОШИБКА: {err}", parent.display());
                return Outcome::Failed;
            }
        }
    }
    if let Err(err) = std::fs::write(&target, &blob) {
        eprintln!("{}: ОШИБКА: {err}", target.display());
        return Outcome::Failed;
    }
    println!("{}", describe(source, &target, &stats));
    Outcome::Written
}

/// Строка отчёта — тот же формат, что в Python-эталоне.
pub fn describe(source: &Path, target: &Path, stats: &rcs_cloud::Stats) -> String {
    let words = if stats.metadata_words.is_empty() {
        String::new()
    } else {
        let joined: Vec<String> = stats.metadata_words.iter().map(u32::to_string).collect();
        format!("generated {} ({} записей)", joined.join("/"), stats.metadata_records)
    };
    format!(
        "{} → {}: точек {}, узлов {} (глубина {}, ребро {:.3} м), макс. {}/узел, \
         {:.2} МБ, type 1: {}",
        source.display(),
        target.display(),
        stats.points,
        stats.tiles,
        stats.depth,
        stats.tile_edge,
        stats.max_points_per_tile,
        stats.size as f64 / 1e6,
        words
    )
}


/// Совпадает ли расширение файла (регистр не важен).
pub fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| format!(".{value}").eq_ignore_ascii_case(suffix))
}

/// Имя выхода: то же имя с расширением `.rcs` (и, если задан, в `out_dir`).
pub fn default_target(source: &Path, out_dir: Option<&Path>) -> PathBuf {
    let name = source
        .file_stem()
        .map(|stem| format!("{}.rcs", stem.to_string_lossy()))
        .unwrap_or_else(|| "output.rcs".to_string());
    match out_dir {
        Some(dir) => dir.join(name),
        None => source.with_file_name(name),
    }
}

/// Развернуть аргументы CLI в список входных файлов с нужным расширением.
///
/// Принимает файлы, каталоги (все файлы с расширением внутри) и маски.
pub fn expand_sources(patterns: &[String], suffixes: &[&str], recursive: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let path = Path::new(pattern);
        if path.is_dir() {
            collect_dir(path, suffixes, recursive, &mut found);
            continue;
        }
        if pattern.contains('*') || pattern.contains('?') {
            let mut matches = glob(pattern);
            matches.sort();
            if matches.is_empty() {
                found.push(path.to_path_buf());
            } else {
                found.extend(matches);
            }
            continue;
        }
        found.push(path.to_path_buf());
    }

    let mut unique: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for path in found {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        unique.push(path);
    }
    unique
}

/// Файлы каталога с нужными расширениями (сортировка по пути).
fn collect_dir(dir: &Path, suffixes: &[&str], recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if suffixes.iter().any(|suffix| has_suffix(&path, suffix)) {
            files.push(path);
        }
    }
    files.sort();
    out.extend(files);
    if recursive {
        dirs.sort();
        for sub in dirs {
            collect_dir(&sub, suffixes, true, out);
        }
    }
}

/// Маска в одном компоненте пути: `*` — любая подстрока, `?` — один символ.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    match pattern.chars().next() {
        None => text.is_empty(),
        Some('*') => {
            let rest = &pattern[1..];
            (0..=text.len())
                .filter(|index| text.is_char_boundary(*index))
                .any(|index| wildcard_match(rest, &text[index..]))
        }
        Some('?') => match text.chars().next() {
            Some(first) => wildcard_match(&pattern[1..], &text[first.len_utf8()..]),
            None => false,
        },
        Some(other) => match text.chars().next() {
            Some(first) if first == other => {
                wildcard_match(&pattern[other.len_utf8()..], &text[first.len_utf8()..])
            }
            _ => false,
        },
    }
}

/// Развернуть маску в список существующих файлов (каталог + имя с маской).
fn glob(pattern: &str) -> Vec<PathBuf> {
    let path = Path::new(pattern);
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let Some(mask) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| wildcard_match(mask, name))
        })
        .map(|entry| entry.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.laz", "level_7.laz"));
        assert!(!wildcard_match("*.laz", "level_7.las"));
        assert!(wildcard_match("level_?", "level_7"));
        assert!(wildcard_match("*", "anything"));
        assert!(!wildcard_match("a*b", "ab_c"));
        assert!(wildcard_match("a*b", "aXXb"));
    }

    #[test]
    fn target_name() {
        assert_eq!(
            default_target(Path::new("scan.laz"), None),
            PathBuf::from("scan.rcs")
        );
        assert_eq!(
            default_target(Path::new("scan.laz"), Some(Path::new("out"))),
            PathBuf::from("out").join("scan.rcs")
        );
    }

    #[test]
    fn suffixes_ignore_case() {
        assert!(has_suffix(Path::new("a.LAZ"), ".laz"));
        assert!(!has_suffix(Path::new("a.las"), ".laz"));
    }
}

