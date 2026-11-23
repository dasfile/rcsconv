//! Container serialisation: `Container` -> bytes.
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 9): `read -> write` produces a byte-identical
//! file on every reference file. Layout (little-endian, no alignment):
//!
//! ```text
//! 0x000 .. 0x135   header (see `header::Header`)
//! 0x135            directory: blockCount records (offset u64, size u64, type u64)
//! .. 0x1812D       zeros: directory terminator and reserve
//! 0x1812D .. EOF   blocks back to back, in directory order
//! ```
//!
//! Blocks `type 3` and `type 11` are rebuilt from the parsed structures; the
//! others are copied as is.

use crate::container::Container;
use crate::directory::{DIR_RECORD_SIZE, FIRST_BLOCK_OFFSET};
use crate::error::{Error, Result};
use crate::header::Header;
use crate::tile::{iter_tiles, tile_block_bytes, tile_payload};

/// Rebuild the `type 3`/`type 11` blocks, copy the rest as is.
///
/// This is exactly what the Python reference does (`rcs_write.default_blocks`),
/// so the result must match byte for byte.
pub fn default_blocks(container: &Container<'_>) -> Result<Vec<Vec<u8>>> {
    let tiles = iter_tiles(container)?;
    let mut points = Vec::new();
    for tile in &tiles {
        points.extend_from_slice(tile_payload(container, tile)?);
    }
    let tile_block = tile_block_bytes(&tiles)?;

    let mut blocks = Vec::with_capacity(container.blocks().len());
    for block in container.blocks() {
        blocks.push(match block.kind {
            3 => tile_block.clone(),
            11 => points.clone(),
            _ => container.read_block(*block).to_vec(),
        });
    }
    Ok(blocks)
}

/// Build a file from scratch: header, directory, reserve, blocks.
///
/// `blocks` are `(type, contents)` pairs in directory order: this is how the
/// writer builds a `.rcs` without a template — every block is derived from the
/// specification.
pub fn build_file(
    header: &Header,
    blocks: &[(u64, Vec<u8>)],
    guid: Option<&str>,
) -> Result<Vec<u8>> {
    if blocks.is_empty() {
        return Err(Error::NoBlocks);
    }
    let dir_end = crate::header::HEADER_END + DIR_RECORD_SIZE * blocks.len();
    if dir_end > FIRST_BLOCK_OFFSET {
        return Err(Error::DirectoryOverflow {
            dir_end,
            first_block: FIRST_BLOCK_OFFSET,
        });
    }

    let mut out = header.to_bytes(guid, blocks.len())?;
    let mut offset = FIRST_BLOCK_OFFSET as u64;
    for (kind, chunk) in blocks {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(chunk.len() as u64).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        offset += chunk.len() as u64;
    }
    out.resize(FIRST_BLOCK_OFFSET, 0);
    for (_, chunk) in blocks {
        out.extend_from_slice(chunk);
    }
    Ok(out)
}

/// Serialise a container.
///
/// `blocks` replaces block contents in directory order (`None` rebuilds
/// `type 3`/`type 11` and copies the rest); `guid` replaces the GUID string
/// (`None` keeps the original one, as in the Python reference).
pub fn serialize(
    container: &Container<'_>,
    blocks: Option<&[Vec<u8>]>,
    guid: Option<&str>,
) -> Result<Vec<u8>> {
    let data = match blocks {
        Some(list) => list.to_vec(),
        None => default_blocks(container)?,
    };
    if data.len() != container.blocks().len() {
        return Err(Error::BlockCount {
            got: data.len(),
            expected: container.blocks().len(),
        });
    }
    let typed: Vec<(u64, Vec<u8>)> = container
        .blocks()
        .iter()
        .zip(data)
        .map(|(block, chunk)| (block.kind, chunk))
        .collect();
    build_file(container.header(), &typed, guid)
}

/// Contiguous ranges `(start, end inclusive)` where the bytes differ.
pub fn diff_ranges(left: &[u8], right: &[u8]) -> Result<Vec<(usize, usize)>> {
    if left.len() != right.len() {
        return Err(Error::SizeMismatch {
            left: left.len(),
            right: right.len(),
        });
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, (a, b)) in left.iter().zip(right).enumerate() {
        if a == b {
            continue;
        }
        match ranges.last_mut() {
            Some((_, high)) if *high + 1 == index => *high = index,
            _ => ranges.push((index, index)),
        }
    }
    Ok(ranges)
}
