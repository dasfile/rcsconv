//! Block `type 1` — octree metadata.
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 4.3, `tools/analyze_type1.py`): a 128-byte
//! header (32 × `u32`) followed by 72-byte records.
//! `words[L]` is the number of occupied tiles at level `L` (`words[0] = 1` is the
//! root), `K = sum(words)` is the total number of tiles (= number of records).
//! All records are identical: an empty attribute range (`±DOUBLE_MAX`).
//! HYPOTHESIS: one record corresponds to one octree tile.

use crate::bytes;
use crate::error::{Error, Result};

/// Number of `u32` in the block header.
pub const METADATA_WORD_COUNT: usize = 32;
/// Size of the block header.
pub const METADATA_HEAD_SIZE: usize = 4 * METADATA_WORD_COUNT;
/// Size of one record.
pub const METADATA_RECORD_SIZE: usize = 72;
/// "No bound" placeholder in a record.
pub const DOUBLE_MAX: f64 = f64::MAX;

/// Parsed metadata block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// 32 header words (occupied tiles per level, zeros after that).
    pub words: [u32; METADATA_WORD_COUNT],
    /// Number of records after the header.
    pub record_count: usize,
    /// Tail not divisible by 72 bytes (0 in the reference files) — kept as a fact.
    pub trailing: usize,
}

impl Metadata {
    /// Parse a `type 1` block.
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

    /// Total number of octree tiles `K = sum(words)`.
    pub fn occupied_nodes(&self) -> u64 {
        self.words.iter().map(|word| *word as u64).sum()
    }

    /// Level words without trailing zeros (`probe6` -> `[1, 3]`).
    pub fn levels(&self) -> Vec<u32> {
        let last = self
            .words
            .iter()
            .rposition(|word| *word != 0)
            .map_or(0, |index| index + 1);
        self.words[..last].to_vec()
    }

    /// Whether the reference invariant `K = sum(words)` holds.
    pub fn is_consistent(&self) -> bool {
        self.occupied_nodes() == self.record_count as u64
    }
}

/// One metadata block record: an empty attribute range (`±DOUBLE_MAX`).
///
/// CONFIRMED: in all 13 reference files the records are identical —
/// `u32 0, u32 0, +DOUBLE_MAX ×3, -DOUBLE_MAX ×3, u32 0, -1, -1, 0`.
pub fn metadata_record() -> [u8; METADATA_RECORD_SIZE] {
    let mut out = [0u8; METADATA_RECORD_SIZE];
    // u32 0, u32 0 (0..8), +DOUBLE_MAX ×3 (8..32), -DOUBLE_MAX ×3 (32..56),
    // u32 0 (56), -1, -1 (60, 64), u32 0 (68).
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

/// Build a `type 1` block from the header words.
///
/// `records = None` -> `sum(words)`: the reference files have no case where
/// `K != sum(words)`.
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
