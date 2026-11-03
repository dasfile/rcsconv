//! Оркестрация конвертации: общая часть CLI (`rcsconv`) и GUI (`rcsconv-gui`).
//!
//! Здесь нет ни печати в консоль, ни окон: слой отдаёт результат
//! ([`Conversion`]), а интерфейс сам решает, куда его показать. Правила
//! интерфейса повторяют Python-эталон (`tools/rcs_cli.py`, README): вход —
//! файл, каталог или маска; имя выхода `<имя>.rcs`; существующий файл не
//! перезаписывается без `--force`.

#![forbid(unsafe_code)]

pub mod convert;
pub mod files;
pub mod options;

pub use convert::{convert_one, describe, write_atomic, Conversion, Failure, Status};
pub use files::{collect_dir, default_target, expand_sources, glob, has_suffix, wildcard_match};
pub use options::{ArgsError, Options};

/// Прогресс и отмена — реэкспорт из `rcs-cloud`, чтобы GUI зависел от одного крейта.
pub use rcs_cloud::progress::{NoProgress, Progress, Stage};
pub use rcs_cloud::{BuildOptions, Stats};

/// Настройки входа LAS/LAZ — GUI строит их из полей окна (как CLI из ключей).
pub use cloud_las::{LasOptions, Recenter, ScaleMode};
