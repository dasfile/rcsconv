//! rcsconv — конвертер Autodesk ReCap `.rcs` (контейнер ADOCT).
//!
//! Состояние: фаза R0 — каркас CLI и проверка сборки статического бинарника
//! (`docs/ROADMAP.md` §4). Подкоманды появятся в R1–R4; цель — побайтовый
//! паритет с Python-реализацией в `tools/`.

use std::process::ExitCode;

const USAGE: &str = "\
rcsconv — reader/writer for Autodesk ReCap .rcs (ADOCT)

Usage:
  rcsconv --version   print version
  rcsconv --help      print this help

Статус: каркас (фаза R0). Команды pts2rcs / las2rcs / rcs2pts / inspect
появятся в фазах R1–R4 (docs/ROADMAP.md).
";

/// Разобранная командная строка (пока без подкоманд — они появятся в R1–R4).
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    Help,
    Unknown(String),
}

fn parse(args: &[String]) -> Command {
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => Command::Version,
        Some("--help") | Some("-h") => Command::Help,
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
        assert_eq!(parse(&args(&["-h"])), Command::Help);
        assert_eq!(parse(&[]), Command::Help);
    }

    #[test]
    fn unknown_argument_is_reported() {
        assert_eq!(
            parse(&args(&["pts2rcs"])),
            Command::Unknown("pts2rcs".to_string())
        );
    }

    #[test]
    fn usage_mentions_binary_name() {
        assert!(USAGE.contains("rcsconv"));
    }
}

