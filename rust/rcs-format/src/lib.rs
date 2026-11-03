//! Контейнер Autodesk ReCap `.rcs` (сигнатура `ADOCT`) — слой формата.
//!
//! Порт `tools/rcs_blocks.py`, `tools/rcs_points.py`, `tools/rcs_write.py`
//! (эталон — Python-реализация; паритет проверяется побайтово, `docs/ROADMAP.md` R1).
//!
//! Подтверждено байтами эталонов (`docs/RCS-FORMAT.md`):
//!
//! * заголовок `0x000…0x135` — сигнатура, центр bbox, корневой куб, `chunk_version`,
//!   `count`, `flags`, строка GUID `{8_4_4_4_12}` на `0xE3`, `blockCount` на `0x129`;
//! * директория блоков на `0x135` — записи `(offset u64, size u64, type u64)`,
//!   блоки стыкуются и заканчиваются ровно на EOF (выравнивания нет);
//! * блок `type 3` — `u64 count` + `count × 368` записей узлов;
//! * блок `type 11` — payload'ы узлов подряд, тело точки 16 байт (квантование по осям);
//! * блок `type 1` — 128 байт заголовка (32 × u32) + записи по 72 байта;
//! * блок `type 0` (экспорт) — TLV с версией схемы атрибутов.
//!
//! Публичный API повторяет питоновский: `Container::parse` → `iter_tiles` →
//! `decode_tile`; сборка — `writer::serialize`.

#![forbid(unsafe_code)]

mod bytes;

pub mod container;
pub mod directory;
pub mod error;
pub mod header;
pub mod metadata;
pub mod num;
pub mod point;
pub mod properties;
pub mod tile;
pub mod tlv;
pub mod writer;

pub use container::Container;
pub use directory::{Block, FIRST_BLOCK_OFFSET};
pub use error::{Error, Result};
pub use header::Header;
pub use metadata::Metadata;
pub use point::Point;
pub use tile::Tile;
pub use tlv::Tlv;

/// Сигнатура контейнера: `ADOCT` + три нуля.
pub const MAGIC: [u8; 8] = *b"ADOCT\0\0\0";
