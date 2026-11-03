//! CLI-слой: разбор аргументов и печать отчётов.
//!
//! Правило интерфейса повторяет Python-эталон (`tools/rcs_cli.py`, README):
//! первый аргумент — вход (файл, каталог или маска), имя выхода подставляется
//! само (`<имя>.rcs`), существующий файл не перезаписывается без `-f`.
//!
//! Логика (разбор ключей, обход входов, конвертация, атомарная запись) живёт
//! в крейте `rcs-conv` — её же использует GUI (`rcsconv-gui`).

use rcs_conv::{convert_one, expand_sources, has_suffix, NoProgress, Options, Status};

/// Пройти по входам и собрать `.rcs`; вернуть код выхода.
pub fn run(command: &str, args: &[String]) -> i32 {
    let las = command == "las2rcs";
    let options = match Options::parse(args, las) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("rcsconv {command}: {}", err.0);
            return 2;
        }
    };
    if let Err(message) = options.validate() {
        eprintln!("rcsconv {command}: {message}");
        return 2;
    }
    let build_options = match options.build_options() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("rcsconv {command}: {message}");
            return 2;
        }
    };

    let sources = expand_sources(&options.sources, Options::suffixes(las), options.recursive);
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
        let result = convert_one(source, &options, &build_options, las, &NoProgress);
        match result.status {
            Status::Written => {
                written += 1;
                println!("{}", result.message());
            }
            Status::Skipped => {
                skipped += 1;
                println!("{}", result.message());
            }
            Status::Failed | Status::Cancelled => {
                failed += 1;
                eprintln!("{}", result.message());
            }
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
