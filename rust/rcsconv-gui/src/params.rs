//! Параметры конвертации, введённые в окне.
//!
//! Задача модуля — превратить состояние полей в [`rcs_conv::Options`] точно так
//! же, как CLI разбирает ключи (`Options::parse`), и сохранить/прочитать их из
//! реестра. Win32 здесь нет, поэтому равенство с CLI проверяется тестом.

use std::path::PathBuf;

use rcs_conv::{LasOptions, Options, Recenter};

/// Как центрировать координаты (`--recenter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecenterChoice {
    /// Не сдвигать.
    None,
    /// Вычесть центр bbox.
    Center,
    /// Вычесть минимум bbox.
    Min,
}

impl RecenterChoice {
    /// Все варианты в порядке выпадающего списка.
    pub const ALL: [RecenterChoice; 3] =
        [RecenterChoice::None, RecenterChoice::Center, RecenterChoice::Min];

    /// Подпись в интерфейсе.
    pub fn label(self) -> &'static str {
        match self {
            RecenterChoice::None => "не сдвигать",
            RecenterChoice::Center => "по центру",
            RecenterChoice::Min => "по минимуму",
        }
    }

    /// Значение ключа `--recenter`.
    pub fn cli_value(self) -> &'static str {
        match self {
            RecenterChoice::None => "none",
            RecenterChoice::Center => "center",
            RecenterChoice::Min => "min",
        }
    }

    /// Разобрать значение ключа.
    pub fn from_cli(text: &str) -> Option<RecenterChoice> {
        RecenterChoice::ALL
            .into_iter()
            .find(|choice| choice.cli_value().eq_ignore_ascii_case(text))
    }

    fn to_recenter(self) -> Recenter {
        match self {
            RecenterChoice::None => Recenter::None,
            RecenterChoice::Center => Recenter::Center,
            RecenterChoice::Min => Recenter::Min,
        }
    }
}

/// Предел точек в узле по умолчанию (предел формата).
pub const DEFAULT_MAX_TILE: &str = "65536";

/// Параметры конвертации в текстовом виде — как они введены в окне.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Папка вывода (`-o`); пусто — рядом с входом.
    pub out_dir: String,
    /// Множитель координат (`--units-scale`).
    pub units: String,
    /// Центрирование (`--recenter`).
    pub recenter: RecenterChoice,
    /// Предел точек в узле (`-m`).
    pub max_per_tile: String,
    /// GUID результата (`-g`); пусто — случайный.
    pub guid: String,
    /// Читать цвет LAS.
    pub color: bool,
    /// Читать интенсивность LAS.
    pub intensity: bool,
    /// Перезаписывать существующий `.rcs` (`-f`).
    pub force: bool,
    /// Обходить подкаталоги (`-r`).
    pub recursive: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            out_dir: String::new(),
            units: "1".to_string(),
            recenter: RecenterChoice::None,
            max_per_tile: DEFAULT_MAX_TILE.to_string(),
            guid: String::new(),
            color: true,
            intensity: true,
            force: false,
            recursive: false,
        }
    }
}

impl Settings {
    /// Проверить значения и собрать [`Options`] для набора входов.
    ///
    /// Сообщения об ошибках совпадают с CLI: GUI показывает их в подсказке.
    pub fn to_options(&self, sources: Vec<String>) -> Result<Options, String> {
        let units: f64 = self
            .units
            .trim()
            .parse()
            .map_err(|_| format!("--units-scale {}: ожидается число", self.units.trim()))?;
        if units <= 0.0 {
            return Err("--units-scale должен быть > 0".to_string());
        }
        let max_per_tile: u32 = self
            .max_per_tile
            .trim()
            .parse()
            .map_err(|_| format!("-m {}: ожидается целое число", self.max_per_tile.trim()))?;
        let out_dir = self.out_dir.trim();
        let guid = self.guid.trim();

        let options = Options {
            sources,
            out_dir: if out_dir.is_empty() { None } else { Some(PathBuf::from(out_dir)) },
            max_per_tile,
            force: self.force,
            recursive: self.recursive,
            guid: if guid.is_empty() { None } else { Some(guid.to_string()) },
            las: LasOptions {
                units,
                recenter: self.recenter.to_recenter(),
                color: self.color,
                intensity: self.intensity,
                color_scale: rcs_conv::ScaleMode::Auto,
                intensity_scale: rcs_conv::ScaleMode::Auto,
            },
        };
        options.validate()?;
        Ok(options)
    }

    /// Проверить значения без сборки [`Options`].
    pub fn validate(&self) -> Result<(), String> {
        self.to_options(vec!["проверка".to_string()]).map(|_| ())
    }

    /// Пары «имя, значение» для реестра.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("out_dir".to_string(), self.out_dir.clone()),
            ("units".to_string(), self.units.clone()),
            ("recenter".to_string(), self.recenter.cli_value().to_string()),
            ("max_per_tile".to_string(), self.max_per_tile.clone()),
            ("guid".to_string(), self.guid.clone()),
            ("color".to_string(), flag(self.color)),
            ("intensity".to_string(), flag(self.intensity)),
            ("force".to_string(), flag(self.force)),
            ("recursive".to_string(), flag(self.recursive)),
        ]
    }

    /// Восстановить параметры из пар реестра (неизвестные имена игнорируются).
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Settings {
        let mut settings = Settings::default();
        for (name, value) in pairs {
            match name {
                "out_dir" => settings.out_dir = value.to_string(),
                "units" => settings.units = value.to_string(),
                "recenter" => {
                    if let Some(choice) = RecenterChoice::from_cli(value) {
                        settings.recenter = choice;
                    }
                }
                "max_per_tile" => settings.max_per_tile = value.to_string(),
                "guid" => settings.guid = value.to_string(),
                "color" => settings.color = truthy(value),
                "intensity" => settings.intensity = truthy(value),
                "force" => settings.force = truthy(value),
                "recursive" => settings.recursive = truthy(value),
                _ => {}
            }
        }
        settings
    }
}

/// Флаг в реестр: `1` / `0`.
fn flag(value: bool) -> String {
    if value {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

/// Флаг из реестра: `1`/`true`/`yes`/`on` — истина, остальное — ложь.
fn truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Options {
        let args: Vec<String> = args.iter().map(|item| item.to_string()).collect();
        Options::parse(&args, true).expect("разбор CLI")
    }

    /// Поля `Options`, которые задаёт GUI, должны совпасть с CLI.
    fn assert_same(gui: &Options, cli: &Options) {
        assert_eq!(gui.sources, cli.sources);
        assert_eq!(gui.out_dir, cli.out_dir);
        assert_eq!(gui.max_per_tile, cli.max_per_tile);
        assert_eq!(gui.force, cli.force);
        assert_eq!(gui.recursive, cli.recursive);
        assert_eq!(gui.guid, cli.guid);
        assert_eq!(gui.las.units, cli.las.units);
        assert_eq!(gui.las.recenter, cli.las.recenter);
        assert_eq!(gui.las.color, cli.las.color);
        assert_eq!(gui.las.intensity, cli.las.intensity);
    }

    #[test]
    fn defaults_match_bare_cli() {
        let gui = Settings::default().to_options(vec!["a.laz".to_string()]).expect("gui");
        assert_same(&gui, &cli(&["a.laz"]));
    }

    #[test]
    fn all_fields_match_cli() {
        let settings = Settings {
            out_dir: "out".to_string(),
            units: "0.3048".to_string(),
            recenter: RecenterChoice::Center,
            max_per_tile: "1024".to_string(),
            guid: "{00112233-4455-6677-8899-aabbccddeeff}".to_string(),
            color: false,
            intensity: true,
            force: true,
            recursive: true,
        };
        let gui = settings.to_options(vec!["a.laz".to_string()]).expect("gui");
        let reference = cli(&[
            "a.laz",
            "-o",
            "out",
            "-m",
            "1024",
            "-f",
            "-r",
            "-g",
            "{00112233-4455-6677-8899-aabbccddeeff}",
            "--units-scale",
            "0.3048",
            "--recenter",
            "center",
            "--no-color",
        ]);
        assert_same(&gui, &reference);
    }

    #[test]
    fn earlier_recenter_matches_cli() {
        for (choice, value) in [
            (RecenterChoice::None, "none"),
            (RecenterChoice::Center, "center"),
            (RecenterChoice::Min, "min"),
        ] {
            let settings = Settings { recenter: choice, ..Settings::default() };
            let gui = settings.to_options(vec!["a.laz".to_string()]).expect("gui");
            let reference = cli(&["a.laz", "--recenter", value]);
            assert_eq!(gui.las.recenter, reference.las.recenter, "{value}");
            assert_eq!(RecenterChoice::from_cli(value), Some(choice));
        }
    }

    #[test]
    fn rejects_bad_numbers() {
        let bad = Settings { units: "0".to_string(), ..Settings::default() };
        assert_eq!(bad.validate(), Err("--units-scale должен быть > 0".to_string()));

        let bad = Settings { units: "abc".to_string(), ..Settings::default() };
        assert!(bad.validate().unwrap_err().contains("ожидается число"));

        let bad = Settings { max_per_tile: "0".to_string(), ..Settings::default() };
        assert_eq!(bad.validate(), Err("-m 0 вне 1…65536".to_string()));

        let bad = Settings { max_per_tile: "70000".to_string(), ..Settings::default() };
        assert_eq!(bad.validate(), Err("-m 70000 вне 1…65536".to_string()));

        let bad = Settings { max_per_tile: "много".to_string(), ..Settings::default() };
        assert!(bad.validate().unwrap_err().contains("ожидается целое число"));

        assert_eq!(Settings::default().validate(), Ok(()));
    }

    #[test]
    fn settings_survive_registry_round_trip() {
        let settings = Settings {
            out_dir: "D:\\out dir".to_string(),
            units: "0.3048".to_string(),
            recenter: RecenterChoice::Min,
            max_per_tile: "4096".to_string(),
            guid: "abc".to_string(),
            color: false,
            intensity: false,
            force: true,
            recursive: false,
        };
        let pairs = settings.to_pairs();
        let borrowed: Vec<(&str, &str)> =
            pairs.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        assert_eq!(Settings::from_pairs(borrowed), settings);

        // Пустой набор пар — значения по умолчанию.
        assert_eq!(Settings::from_pairs(Vec::new()), Settings::default());
    }
}
