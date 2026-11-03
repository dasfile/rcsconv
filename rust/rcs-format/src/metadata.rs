//! Блок `type 1` — метаданные октодерева.
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §4.3, `tools/analyze_type1.py`):
//! 128 байт заголовка (32 × `u32`), дальше записи по 72 байта.
//! `words[L]` — число занятых узлов на уровне `L` (`words[0] = 1` — корень),
//! `K = sum(words)` — полное число узлов (= число записей блока).
//! Все записи идентичны: пустой диапазон атрибута (`±DOUBLE_MAX`).
//! ГИПОТЕЗА: запись соответствует одному узлу октодерева.

use crate::bytes;
use crate::error::{Error, Result};

/// Число `u32` в заголовке блока.
pub const METADATA_WORD_COUNT: usize = 32;
/// Размер заголовка блока.
pub const METADATA_HEAD_SIZE: usize = 4 * METADATA_WORD_COUNT;
/// Размер одной записи.
pub const METADATA_RECORD_SIZE: usize = 72;
/// Заглушка «нет границы» в записи.
pub const DOUBLE_MAX: f64 = f64::MAX;

/// Разобранный блок метаданных.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// 32 слова заголовка (занятые узлы по уровням, дальше нули).
    pub words: [u32; METADATA_WORD_COUNT],
    /// Число записей после заголовка.
    pub record_count: usize,
    /// Хвост, не кратный 72 байтам (в эталонах 0) — сохраняется как факт.
    pub trailing: usize,
}

impl Metadata {
    /// Разобрать блок `type 1`.
    pub fn parse(block: &[u8]) -> Result<Metadata> {
        if block.len() < METADATA_HEAD_SIZE {
            return Err(Error::TooShort {
                size: block.len(),
                need: METADATA_HEAD_SIZE,
            });
        }
        let mut words = [0u32; METADATA_WORD_COUNT];
        for (index, slot) in words.iter_mut().enumerate() {
            *slot = bytes::u32_at(block, index * 4)?;
        }
        let body = block.len() - METADATA_HEAD_SIZE;
        Ok(Metadata {
            words,
            record_count: body / METADATA_RECORD_SIZE,
            trailing: body % METADATA_RECORD_SIZE,
        })
    }

    /// Полное число узлов октодерева `K = sum(words)`.
    pub fn occupied_nodes(&self) -> u64 {
        self.words.iter().map(|word| *word as u64).sum()
    }

    /// Слова уровней без хвостовых нулей (`probe6` → `[1, 3]`).
    pub fn levels(&self) -> Vec<u32> {
        let last = self
            .words
            .iter()
            .rposition(|word| *word != 0)
            .map_or(0, |index| index + 1);
        self.words[..last].to_vec()
    }

    /// Выполняется ли инвариант эталонов `K = sum(words)`.
    pub fn is_consistent(&self) -> bool {
        self.occupied_nodes() == self.record_count as u64
    }
}

/// Одна запись блока метаданных: пустой диапазон атрибута (`±DOUBLE_MAX`).
///
/// ПОДТВЕРЖДЕНО: во всех 13 эталонах записи идентичны —
/// `u32 0, u32 0, +DOUBLE_MAX ×3, −DOUBLE_MAX ×3, u32 0, −1, −1, 0`.
pub fn metadata_record() -> [u8; METADATA_RECORD_SIZE] {
    let mut out = [0u8; METADATA_RECORD_SIZE];
    // u32 0, u32 0 (0…8), +DOUBLE_MAX ×3 (8…32), −DOUBLE_MAX ×3 (32…56),
    // u32 0 (56), −1, −1 (60, 64), u32 0 (68).
    bytes::put_f64(&mut out, 8, DOUBLE_MAX);
    bytes::put_f64(&mut out, 16, DOUBLE_MAX);
    bytes::put_f64(&mut out, 24, DOUBLE_MAX);
    bytes::put_f64(&mut out, 32, -DOUBLE_MAX);
    bytes::put_f64(&mut out, 40, -DOUBLE_MAX);
    bytes::put_f64(&mut out, 48, -DOUBLE_MAX);
    bytes::put_u32(&mut out, 56, 0);
    bytes::put_u32(&mut out, 60, u32::MAX);
    bytes::put_u32(&mut out, 64, u32::MAX);
    bytes::put_u32(&mut out, 68, 0);
    out
}

/// Собрать блок `type 1` из слов заголовка.
///
/// `records = None` → `sum(words)`: в эталонах нет примера, где `K ≠ sum(words)`.
pub fn build_metadata_block(words: &[u32], records: Option<usize>) -> Result<Vec<u8>> {
    if words.len() > METADATA_WORD_COUNT {
        return Err(Error::MetadataWords(words.len()));
    }
    let sum: u32 = words.iter().copied().sum();
    let records = match records {
        Some(given) if given as u32 != sum => {
            return Err(Error::MetadataRecords { records: given, sum });
        }
        Some(given) => given,
        None => sum as usize,
    };

    let mut out = vec![0u8; METADATA_HEAD_SIZE + METADATA_RECORD_SIZE * records];
    for (index, word) in words.iter().enumerate() {
        bytes::put_u32(&mut out, index * 4, *word);
    }
    let record = metadata_record();
    for index in 0..records {
        let start = METADATA_HEAD_SIZE + index * METADATA_RECORD_SIZE;
        out[start..start + METADATA_RECORD_SIZE].copy_from_slice(&record);
    }
    Ok(out)
}
