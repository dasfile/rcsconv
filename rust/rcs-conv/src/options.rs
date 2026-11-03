//! Разбор параметров конвертации — общий для CLI и GUI.
//!
//! Правило интерфейса повторяет Python-эталон (`tools/rcs_cli.py`, README):
//! первый аргумент — вход (файл, каталог или маска), имя выхода подставляется
//! само (`<имя>.rcs`), существующий файл не перезаписывается без `-f`.

use std::path::PathBuf;

use rcs_cloud::BuildOptions;

/// Расширения входа для `las2rcs`.
pub const LAS_SUFFIXES: &[&str] = &[".las", ".laz"];
/// Расширения входа для `pts2rcs`.
pub const PTS_SUFFIXES: &[&str] = &[".pts"];

/// Общие параметры конвертеров.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsError(pub String);

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ArgsError {}

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
                    options.las.recenter = cloud_las::Recenter::parse(&text).ok_or_else(|| {
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

    /// Проверить значения, которые не ловятся при разборе (диапазон `-m`, входы).
    ///
    /// Сообщения совпадают с выводом CLI, чтобы GUI показывал то же самое.
    pub fn validate(&self) -> Result<(), String> {
        if self.sources.is_empty() {
            return Err("не указаны входные файлы".to_string());
        }
        if self.max_per_tile == 0 || self.max_per_tile > rcs_cloud::MAX_TILE_POINTS {
            return Err(format!(
                "-m {} вне 1…{}",
                self.max_per_tile,
                rcs_cloud::MAX_TILE_POINTS
            ));
        }
        Ok(())
    }

    /// Параметры сборки: нормализованный GUID и предел точек в узле.
    pub fn build_options(&self) -> Result<BuildOptions, String> {
        let guid = match &self.guid {
            Some(text) => Some(rcs_cloud::normalize_guid(text).map_err(|err| err.to_string())?),
            None => None,
        };
        Ok(BuildOptions { max_per_tile: self.max_per_tile, guid })
    }

    /// Расширения входа для режима (`las2rcs` или `pts2rcs`).
    pub fn suffixes(las: bool) -> &'static [&'static str] {
        if las {
            LAS_SUFFIXES
        } else {
            PTS_SUFFIXES
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parses_common_flags() {
        let options = Options::parse(
            &args(&["a.laz", "b.laz", "-o", "out", "-m", "1024", "-f", "-r", "-g", "abc"]),
            true,
        )
        .expect("разбор");
        assert_eq!(options.sources, vec!["a.laz".to_string(), "b.laz".to_string()]);
        assert_eq!(options.out_dir, Some(PathBuf::from("out")));
        assert_eq!(options.max_per_tile, 1024);
        assert!(options.force);
        assert!(options.recursive);
        assert_eq!(options.guid.as_deref(), Some("abc"));
    }

    #[test]
    fn parses_las_flags_only_for_las() {
        let options =
            Options::parse(&args(&["a.laz", "--units-scale", "0.3048"]), true).expect("разбор");
        assert!((options.las.units - 0.3048).abs() < 1e-12);

        let error = Options::parse(&args(&["a.pts", "--units-scale", "0.3048"]), false)
            .expect_err("для .pts ключ LAS не разрешён");
        assert!(error.0.contains("неизвестный аргумент"), "{error}");
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Options::parse(&args(&["a.laz", "--units-scale", "0"]), true).is_err());
        assert!(Options::parse(&args(&["a.laz", "--recenter", "somewhere"]), true).is_err());
        assert!(Options::parse(&args(&["a.laz", "-m", "many"]), true).is_err());
        assert!(Options::parse(&args(&["a.laz", "--wobble"]), true).is_err());
        assert!(Options::parse(&args(&["--out-dir"]), false).is_err());
    }

    #[test]
    fn validate_reports_like_cli() {
        let options = Options::default();
        assert_eq!(options.validate(), Err("не указаны входные файлы".to_string()));

        let options =
            Options { sources: vec!["a.laz".into()], max_per_tile: 0, ..Options::default() };
        assert_eq!(options.validate(), Err("-m 0 вне 1…65536".to_string()));

        let options = Options {
            sources: vec!["a.laz".into()],
            max_per_tile: rcs_cloud::MAX_TILE_POINTS + 1,
            ..Options::default()
        };
        assert_eq!(options.validate(), Err("-m 65537 вне 1…65536".to_string()));

        let options = Options { sources: vec!["a.laz".into()], ..Options::default() };
        assert_eq!(options.validate(), Ok(()));
    }

    #[test]
    fn build_options_normalizes_guid() {
        let options = Options {
            guid: Some("{00112233-4455-6677-8899-aabbccddeeff}".into()),
            ..Options::default()
        };
        let build = options.build_options().expect("guid");
        assert_eq!(build.guid.as_deref(), Some("{00112233_4455_6677_8899_aabbccddeeff}"));

        let options = Options { guid: Some("nope".into()), ..Options::default() };
        assert!(options.build_options().is_err());
    }
}
