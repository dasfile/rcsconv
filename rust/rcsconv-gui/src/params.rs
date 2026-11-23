//! Conversion parameters entered in the window.
//!
//! The module turns the field state into [`rcs_conv::Options`] exactly the way the
//! CLI parses flags (`Options::parse`), and saves/loads them from the registry.
//! There is no Win32 here, so a test checks equality with the CLI.

use std::path::PathBuf;

use rcs_conv::{LasOptions, Options, Recenter};

/// How to recentre coordinates (`--recenter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecenterChoice {
    /// Do not shift.
    None,
    /// Subtract the bbox centre.
    Center,
    /// Subtract the bbox minimum.
    Min,
}

impl RecenterChoice {
    /// All variants in combo box order.
    pub const ALL: [RecenterChoice; 3] =
        [RecenterChoice::None, RecenterChoice::Center, RecenterChoice::Min];

    /// Label in the interface.
    pub fn label(self) -> &'static str {
        match self {
            RecenterChoice::None => "keep as is",
            RecenterChoice::Center => "bbox center",
            RecenterChoice::Min => "bbox minimum",
        }
    }

    /// Value of the `--recenter` flag.
    pub fn cli_value(self) -> &'static str {
        match self {
            RecenterChoice::None => "none",
            RecenterChoice::Center => "center",
            RecenterChoice::Min => "min",
        }
    }

    /// Parse a flag value.
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

/// Default points per tile (the format limit).
pub const DEFAULT_MAX_TILE: &str = "65536";

/// Conversion parameters as text — exactly as entered in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Output folder (`-o`); empty means next to the input.
    pub out_dir: String,
    /// Coordinate multiplier (`--units-scale`).
    pub units: String,
    /// Recentring (`--recenter`).
    pub recenter: RecenterChoice,
    /// Points per tile (`-m`).
    pub max_per_tile: String,
    /// Result GUID (`-g`); empty means random.
    pub guid: String,
    /// Read LAS colour.
    pub color: bool,
    /// Read LAS intensity.
    pub intensity: bool,
    /// Overwrite an existing `.rcs` (`-f`).
    pub force: bool,
    /// Walk subdirectories (`-r`).
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
            force: true,
            recursive: false,
        }
    }
}

impl Settings {
    /// Validate the values and build [`Options`] for a set of inputs.
    ///
    /// Error messages match the CLI: the GUI shows them in the hint.
    pub fn to_options(&self, sources: Vec<String>) -> Result<Options, String> {
        let units: f64 = self
            .units
            .trim()
            .parse()
            .map_err(|_| format!("--units-scale {}: a number is expected", self.units.trim()))?;
        if units <= 0.0 {
            return Err("--units-scale must be > 0".to_string());
        }
        let max_per_tile: u32 = self
            .max_per_tile
            .trim()
            .parse()
            .map_err(|_| format!("-m {}: an integer is expected", self.max_per_tile.trim()))?;
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
            in_memory: false,
            memory_budget_mb: 2048,
        };
        options.validate()?;
        Ok(options)
    }

    /// Validate the values without building [`Options`].
    pub fn validate(&self) -> Result<(), String> {
        self.to_options(vec!["validation".to_string()]).map(|_| ())
    }

    /// Name/value pairs for the registry.
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

    /// Restore the parameters from registry pairs (unknown names are ignored).
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

/// Flag to the registry: `1` / `0`.
fn flag(value: bool) -> String {
    if value {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

/// Flag from the registry: `1`/`true`/`yes`/`on` mean true, anything else false.
fn truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Options {
        let args: Vec<String> = args.iter().map(|item| item.to_string()).collect();
        Options::parse(&args, true).expect("CLI parse")
    }

    /// The `Options` fields the GUI sets must match the CLI.
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
        assert_eq!(bad.validate(), Err("--units-scale must be > 0".to_string()));

        let bad = Settings { units: "abc".to_string(), ..Settings::default() };
        assert!(bad.validate().unwrap_err().contains("a number is expected"));

        let bad = Settings { max_per_tile: "0".to_string(), ..Settings::default() };
        assert_eq!(bad.validate(), Err("-m 0 out of 1..65536".to_string()));

        let bad = Settings { max_per_tile: "70000".to_string(), ..Settings::default() };
        assert_eq!(bad.validate(), Err("-m 70000 out of 1..65536".to_string()));

        let bad = Settings { max_per_tile: "many".to_string(), ..Settings::default() };
        assert!(bad.validate().unwrap_err().contains("an integer is expected"));

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

        // An empty pair set means the default values.
        assert_eq!(Settings::from_pairs(Vec::new()), Settings::default());
    }
}
