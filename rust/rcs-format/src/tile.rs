//! Блок `type 3` — записи узлов октодерева по 368 байт.
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §4.1): `u64 count` + `count × 368`;
//! куб узла (0…48), реальный bbox точек (48…96), `count` @96+4, `payload_size`
//! @96+8, сумма `payload_size` равна размеру блока `type 11`.
//! ГИПОТЕЗА: порядок узлов совпадает с порядком payload'ов; массивы после @128
//! не расшифрованы (переносятся как есть).

use crate::bytes;
use crate::container::Container;
use crate::error::{Error, Result};

/// Размер записи узла.
pub const TILE_RECORD_SIZE: usize = 368;
/// Смещение `count` в записи узла.
const RECORD_COUNT_OFFSET: usize = 96 + 4;
/// Смещение `payload_size` в записи узла.
const RECORD_PAYLOAD_SIZE_OFFSET: usize = 96 + 8;
/// Число `u32` в «шапке» записи (после bbox).
const RECORD_HEADER_WORDS: usize = 8;
/// Число `u32` в хвосте записи.
const RECORD_TAIL_WORDS: usize = 60;

/// Запись узла октодерева.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Номер записи в блоке `type 3`.
    pub index: usize,
    /// Куб узла — минимум.
    pub cube_min: [f64; 3],
    /// Куб узла — максимум.
    pub cube_max: [f64; 3],
    /// Реальный bbox точек узла — минимум.
    pub points_min: [f64; 3],
    /// Реальный bbox точек узла — максимум.
    pub points_max: [f64; 3],
    /// Число точек узла.
    pub count: u32,
    /// Размер данных узла в блоке `type 11`.
    pub payload_size: u32,
    /// Смещение payload'а внутри блока `type 11` (накопленная сумма размеров).
    pub payload_offset: usize,
    /// Восемь `u32` шапки записи (`header_u32[1]` = `count`, `[2]` = `payload_size`).
    pub header_u32: [u32; 8],
    /// Шестьдесят `u32` хвоста записи (не расшифрованы).
    pub tail_u32: [u32; 60],
}

/// Разобрать блок `type 3`. Пустой вектор, если блока нет.
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

/// Срез данных узла в блоке `type 11` (порядок узлов — гипотеза, подтверждённая
/// совпадением суммы размеров с размером блока).
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

/// Собрать блок `type 3` из записей узлов.
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
