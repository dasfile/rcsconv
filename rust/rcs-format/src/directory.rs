//! Block directory at `0x135`.
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 3): records `(offset u64, size u64, type u64)`
//! of 24 bytes, terminated by an all-zero record; blocks go in ascending order,
//! are contiguous and end exactly at EOF. There is no alignment, so the start of
//! the directory is found by scanning the window after the GUID.

use crate::bytes;
use crate::error::{Error, Result};
use crate::header::Header;

/// Size of a directory record.
pub const DIR_RECORD_SIZE: usize = 24;
/// Guard against a "directory" made of random bytes.
pub const MAX_BLOCKS: usize = 64;
/// How many bytes after the GUID are tried as the directory start.
pub const DIR_SCAN_WINDOW: usize = 0x40;
/// The first block is always here (confirmed on the reference files).
pub const FIRST_BLOCK_OFFSET: usize = 0x1812D;
/// JPEG preview signature in the raw dialect.
pub const JPEG_SOI: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// A directory record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// Block offset in the file.
    pub offset: u64,
    /// Block size in bytes.
    pub size: u64,
    /// Block type (3 — tiles, 11 — points, 1 — metadata, 0/5 — properties/preview).
    pub kind: u64,
}

impl Block {
    /// End of the block (exclusive).
    pub fn end(&self) -> u64 {
        self.offset + self.size
    }
}

/// Container dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Imported scan: `chunk_version` 1/2/134, has a JPEG preview.
    Raw,
    /// Unified RCS export: `chunk_version` 5, TLV in `type 0`.
    Export,
    /// Not recognised (not an error: a writer schema version may be newer).
    Unknown,
}

/// Read directory records until an all-zero one appears.
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

/// Invariant: blocks are ascending, contiguous and end exactly at EOF.
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

/// Find the directory: the first candidate in the window that covers the whole file.
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

/// Dialect by `chunk_version` (observations: 1 — raw from `.laz`, 2 — raw from
/// `.pts`, 5 — export, 134 — "non-text" import).
pub fn dialect_of(chunk_version: u32) -> Dialect {
    match chunk_version {
        1 | 2 | 134 => Dialect::Raw,
        5 => Dialect::Export,
        _ => Dialect::Unknown,
    }
}

/// Dialect by block layout when `chunk_version` is unknown.
pub fn dialect_of_blocks(types: &[u64]) -> Dialect {
    match types {
        [3, 1, 11, 5, 0] => Dialect::Raw,
        [3, 1, 11, 0] => Dialect::Export,
        _ => Dialect::Unknown,
    }
}

/// Scan preview (`type 0` in the raw dialect).
pub fn is_jpeg(blob: &[u8]) -> bool {
    blob.starts_with(&JPEG_SOI)
}
