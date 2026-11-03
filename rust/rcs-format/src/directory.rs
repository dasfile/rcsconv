//! Директория блоков на `0x135`.
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §3): записи `(offset u64, size u64, type u64)`
//! по 24 байта, терминатор — полностью нулевая запись; блоки идут по возрастанию,
//! стыкуются и заканчиваются ровно на EOF. Выравнивания нет, поэтому начало
//! директории ищется подбором в окне после GUID.

use crate::bytes;
use crate::error::{Error, Result};
use crate::header::Header;

/// Размер записи директории.
pub const DIR_RECORD_SIZE: usize = 24;
/// Предохранитель от «директории» из случайных байт.
pub const MAX_BLOCKS: usize = 64;
/// Сколько байт после GUID пробуем как начало директории.
pub const DIR_SCAN_WINDOW: usize = 0x40;
/// Первый блок всегда здесь (подтверждено на эталонах).
pub const FIRST_BLOCK_OFFSET: usize = 0x1812D;
/// Сигнатура JPEG-превью в raw-диалекте.
pub const JPEG_SOI: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Запись директории.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// Смещение блока в файле.
    pub offset: u64,
    /// Размер блока в байтах.
    pub size: u64,
    /// Тип блока (3 — узлы, 11 — точки, 1 — метаданные, 0/5 — свойства/превью).
    pub kind: u64,
}

impl Block {
    /// Конец блока (не включая).
    pub fn end(&self) -> u64 {
        self.offset + self.size
    }
}

/// Диалект контейнера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Импортированный скан: `chunk_version` 1/2/134, есть JPEG-превью.
    Raw,
    /// Экспорт Unified RCS: `chunk_version` 5, TLV в `type 0`.
    Export,
    /// Не опознан (не ошибка: версия схемы писателя может быть новой).
    Unknown,
}

/// Читать записи директории, пока не встретится полностью нулевая.
fn walk_directory(data: &[u8], start: usize) -> Option<Vec<Block>> {
    let mut blocks = Vec::new();
    let mut offset = start;
    loop {
        if offset.checked_add(DIR_RECORD_SIZE)? > data.len() {
            return None;
        }
        let off = bytes::u64_at(data, offset).ok()?;
        let size = bytes::u64_at(data, offset + 8).ok()?;
        let kind = bytes::u64_at(data, offset + 16).ok()?;
        if off == 0 && size == 0 && kind == 0 {
            break;
        }
        if size == 0 || off < 0x100 || off.checked_add(size)? > data.len() as u64 {
            return None;
        }
        blocks.push(Block { offset: off, size, kind });
        offset += DIR_RECORD_SIZE;
        if blocks.len() > MAX_BLOCKS {
            return None;
        }
    }
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}

/// Инвариант: блоки по возрастанию, стыкуются, кончаются ровно на EOF.
fn directory_is_valid(blocks: &[Block], start: usize, file_size: u64) -> bool {
    let mut previous_end: Option<u64> = None;
    for block in blocks {
        if block.offset < start as u64 {
            return false;
        }
        if previous_end.is_some_and(|end| block.offset != end) {
            return false;
        }
        previous_end = Some(block.end());
    }
    previous_end == Some(file_size)
}

/// Найти директорию: первый кандидат в окне, который покрывает файл целиком.
pub fn find_directory(data: &[u8], header: &Header) -> Result<(usize, Vec<Block>)> {
    let file_size = data.len() as u64;
    let window_end = (header.guid_end() + DIR_SCAN_WINDOW).min(data.len());
    for start in header.guid_end()..window_end {
        if let Some(blocks) = walk_directory(data, start) {
            if directory_is_valid(&blocks, start, file_size) {
                return Ok((start, blocks));
            }
        }
    }
    Err(Error::DirectoryNotFound)
}

/// Диалект по `chunk_version` (наблюдения: 1 — raw из `.laz`, 2 — raw из `.pts`,
/// 5 — экспорт, 134 — «нетекстовый» импорт).
pub fn dialect_of(chunk_version: u32) -> Dialect {
    match chunk_version {
        1 | 2 | 134 => Dialect::Raw,
        5 => Dialect::Export,
        _ => Dialect::Unknown,
    }
}

/// Диалект по составу блоков, если `chunk_version` незнаком.
pub fn dialect_of_blocks(types: &[u64]) -> Dialect {
    match types {
        [3, 1, 11, 5, 0] => Dialect::Raw,
        [3, 1, 11, 0] => Dialect::Export,
        _ => Dialect::Unknown,
    }
}

/// JPEG-превью скана (`type 0` в raw-диалекте).
pub fn is_jpeg(blob: &[u8]) -> bool {
    blob.starts_with(&JPEG_SOI)
}
