//! rcsconv — конвертер Autodesk ReCap `.rcs` (контейнер ADOCT).
//!
//! Целевой продукт — один статический `.exe` без Python (`docs/ROADMAP.md`).
//! Готово: `.las/.laz` → `.rcs` (`las2rcs`), `.pts` → `.rcs` (`pts2rcs`).
//! Паритет с Python-эталоном (`tools/`) проверяется побайтово.

mod cli;

use std::process::ExitCode;

const USAGE: &str = "\
rcsconv — reader/writer for Autodesk ReCap .rcs (ADOCT)

Usage:
  rcsconv las2rcs <inputs...> [options]   .las/.laz -> .rcs
  rcsconv pts2rcs <inputs...> [options]   .pts -> .rcs
  rcsconv --version | --help

Inputs: file, directory or mask (*.laz). Output name is derived (<name>.rcs);
an existing file is not overwritten without -f.

Common options:
  -o, --out-dir DIR              write here instead of next to the input
  -m, --max-points-per-tile N    tile limit, 1..65536 (default 65536)
  -f, --force                    overwrite existing .rcs
  -r, --recursive                walk subdirectories
  -g, --guid GUID                set GUID ({8_4_4_4_12}); default is random

las2rcs options:
      --units-scale F            coordinate multiplier (feet -> m: 0.3048)
      --recenter MODE            none | center | min
      --no-color                 ignore LAS colour
      --no-intensity             ignore LAS intensity
      --color-scale MODE         auto | 257 | 1
      --intensity-scale MODE     auto | 257 | 1

Examples:
  rcsconv las2rcs golden/level_7.laz -o out
  rcsconv las2rcs \"golden/*.laz\" -o out -f
  rcsconv las2rcs scan.las --units-scale 0.3048 --recenter center
  rcsconv pts2rcs golden/probe6.pts -o out
";

/// Разобранная командная строка.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    Help,
    Convert(&'static str),
    Unknown(String),
}

fn parse(args: &[String]) -> Command {
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => Command::Version,
        Some("--help") | Some("-h") | Some("help") => Command::Help,
        Some("las2rcs") => Command::Convert("las2rcs"),
        Some("pts2rcs") => Command::Convert("pts2rcs"),
        Some(other) => Command::Unknown(other.to_string()),
        None => Command::Help,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse(&args) {
        Command::Version => {
            println!("rcsconv {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Convert(command) => {
            let code = cli::run(command, &args[1..]);
            ExitCode::from(code as u8)
        }
        Command::Unknown(arg) => {
            eprintln!("rcsconv: неизвестный аргумент: {arg}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn version_flags() {
        assert_eq!(parse(&args(&["--version"])), Command::Version);
        assert_eq!(parse(&args(&["-V"])), Command::Version);
    }

    #[test]
    fn help_flags_and_empty() {
        assert_eq!(parse(&args(&["--help"])), Command::Help);
        assert_eq!(parse(&args(&["help"])), Command::Help);
        assert_eq!(parse(&[]), Command::Help);
    }

    #[test]
    fn convert_commands() {
        assert_eq!(parse(&args(&["las2rcs", "a.laz"])), Command::Convert("las2rcs"));
        assert_eq!(parse(&args(&["pts2rcs", "a.pts"])), Command::Convert("pts2rcs"));
    }

    #[test]
    fn unknown_argument_is_reported() {
        assert_eq!(
            parse(&args(&["explode"])),
            Command::Unknown("explode".to_string())
        );
    }

    #[test]
    fn usage_mentions_binary_name() {
        assert!(USAGE.contains("rcsconv"));
    }
}
