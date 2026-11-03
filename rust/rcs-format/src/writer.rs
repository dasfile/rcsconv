//! Сериализация контейнера: `Container` → байты.
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §9): `read → write` даёт побайтово
//! идентичный файл на всех эталонах. Раскладка (Little-endian, без выравнивания):
//!
//! ```text
//! 0x000 … 0x135   заголовок (см. `header::Header`)
//! 0x135           директория: blockCount записей (offset u64, size u64, type u64)
//! … 0x1812D       нули: терминатор директории и резерв
//! 0x1812D … EOF   блоки подряд, в порядке директории
//! ```
//!
//! Блоки `type 3` и `type 11` пересобираются из разобранных структур, остальные
//! копируются как есть.

use crate::container::Container;
use crate::directory::{DIR_RECORD_SIZE, FIRST_BLOCK_OFFSET};
use crate::error::{Error, Result};
use crate::header::Header;
use crate::tile::{iter_tiles, tile_block_bytes, tile_payload};

/// Пересобрать блоки `type 3`/`type 11`, остальные скопировать как есть.
///
/// Именно это делает Python-эталон (`rcs_write.default_blocks`), поэтому результат
/// обязан совпадать побайтово.
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

/// Собрать файл с нуля: заголовок, директория, резерв, блоки.
///
/// `blocks` — пары `(type, содержимое)` в порядке директории: так писатель
/// собирает `.rcs` без шаблона, все блоки выводятся из спецификации.
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

/// Сериализовать контейнер.
///
/// `blocks` — подмена содержимого блоков по порядку директории (`None` —
/// пересобрать `type 3`/`type 11` и скопировать остальные); `guid` — подменить
/// строку GUID (`None` — сохранить исходную, как в Python-эталоне).
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

/// Непрерывные интервалы `(начало, конец включительно)`, где байты различаются.
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
