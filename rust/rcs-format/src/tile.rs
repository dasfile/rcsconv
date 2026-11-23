//! Block `type 3` — octree tile records of 368 bytes.
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 4.1): `u64 count` + `count × 368`;
//! tile cube (0..48), real point bbox (48..96), `count` @96+4, `payload_size`
//! @96+8, the sum of `payload_size` equals the size of the `type 11` block.
//! HYPOTHESIS: tile order matches payload order; the arrays after @128 are not
//! decoded (they are carried over as is).

use crate::bytes;
use crate::container::Container;
use crate::error::{Error, Result};

/// Tile record size.
pub const TILE_RECORD_SIZE: usize = 368;
/// Offset of `count` in a tile record.
const RECORD_COUNT_OFFSET: usize = 96 + 4;
/// Offset of `payload_size` in a tile record.
const RECORD_PAYLOAD_SIZE_OFFSET: usize = 96 + 8;
/// Number of `u32` in the record "head" (after the bbox).
const RECORD_HEADER_WORDS: usize = 8;
/// Number of `u32` in the record tail.
const RECORD_TAIL_WORDS: usize = 60;

/// Octree tile record.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Record index in the `type 3` block.
    pub index: usize,
    /// Tile cube — minimum.
    pub cube_min: [f64; 3],
    /// Tile cube — maximum.
    pub cube_max: [f64; 3],
    /// Real bbox of the tile points — minimum.
    pub points_min: [f64; 3],
    /// Real bbox of the tile points — maximum.
    pub points_max: [f64; 3],
    /// Number of points in the tile.
    pub count: u32,
    /// Size of the tile data in the `type 11` block.
    pub payload_size: u32,
    /// Payload offset inside the `type 11` block (accumulated sizes).
    pub payload_offset: usize,
    /// Eight `u32` of the record head (`header_u32[1]` = `count`, `[2]` = `payload_size`).
    pub header_u32: [u32; 8],
    /// Sixty `u32` of the record tail (not decoded).
    pub tail_u32: [u32; 60],
}

/// Parse the `type 3` block. An empty vector when the block is missing.
pub fn iter_tiles(container: &Container<'_>) -> Result<Vec<Tile>> {
    let Some(block) = container.block_of_type(3) else {
        return Ok(Vec::new());
    };
    if block.size < 8 {
        return Ok(Vec::new());
    }

    let base = block.offset as usize;
    let count = bytes::u64_at(container.raw(), base)?;
    let records_end = 8u64
        .checked_add(count.checked_mul(TILE_RECORD_SIZE as u64).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX);
    if records_end > block.size {
        return Err(Error::TileOverflow { count, block_size: block.size });
    }

    let mut tiles = Vec::with_capacity(count as usize);
    let mut payload_offset = 0usize;
    for index in 0..count as usize {
        let offset = base + 8 + index * TILE_RECORD_SIZE;
        let mut header_u32 = [0u32; RECORD_HEADER_WORDS];
        for (index, slot) in header_u32.iter_mut().enumerate() {
            *slot = bytes::u32_at(container.raw(), offset + 96 + 4 * index)?;
        }
        let mut tail_u32 = [0u32; RECORD_TAIL_WORDS];
        for (index, slot) in tail_u32.iter_mut().enumerate() {
            *slot = bytes::u32_at(container.raw(), offset + 128 + 4 * index)?;
        }
        let tile = Tile {
            index,
            cube_min: bytes::f64x3_at(container.raw(), offset)?,
            cube_max: bytes::f64x3_at(container.raw(), offset + 24)?,
            points_min: bytes::f64x3_at(container.raw(), offset + 48)?,
            points_max: bytes::f64x3_at(container.raw(), offset + 72)?,
            count: header_u32[(RECORD_COUNT_OFFSET - 96) / 4],
            payload_size: header_u32[(RECORD_PAYLOAD_SIZE_OFFSET - 96) / 4],
            payload_offset,
            header_u32,
            tail_u32,
        };
        payload_offset += tile.payload_size as usize;
        tiles.push(tile);
    }
    Ok(tiles)
}

/// A slice of tile data in the `type 11` block (tile order is a hypothesis
/// confirmed by the payload size sum matching the block size).
pub fn tile_payload<'a>(container: &'a Container<'a>, tile: &Tile) -> Result<&'a [u8]> {
    let block = container
        .block_of_type(11)
        .ok_or(Error::NoPointsBlock)?;
    let start = block.offset as usize + tile.payload_offset;
    let end = start + tile.payload_size as usize;
    if end > block.end() as usize || end > container.raw().len() {
        return Err(Error::PayloadLayout {
            tile: tile.index,
            size: tile.payload_size as u64,
            count: tile.count,
        });
    }
    Ok(&container.raw()[start..end])
}

/// Build the `type 3` block from tile records.
pub fn tile_block_bytes(tiles: &[Tile]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(8 + TILE_RECORD_SIZE * tiles.len());
    out.extend_from_slice(&(tiles.len() as u64).to_le_bytes());
    for tile in tiles {
        for triple in [tile.cube_min, tile.cube_max, tile.points_min, tile.points_max] {
            for value in triple {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for word in tile.header_u32 {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for word in tile.tail_u32 {
            out.extend_from_slice(&word.to_le_bytes());
        }
        if out.len() - 8 != TILE_RECORD_SIZE * (tile.index + 1) {
            return Err(Error::TileRecord { index: tile.index });
        }
    }
    Ok(out)
}
