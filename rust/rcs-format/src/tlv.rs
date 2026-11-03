//! TLV-поток блока свойств (`type 0` в экспорте, `type 5` в raw).
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §3): цепочка `(id u32, size u32, payload)`;
//! `id = 10` содержит вложенный TLV, остальные id — скаляры. Разбор терпимый:
//! поток читается до первого несоответствия, как в `tools/rcs_blocks.py`.

use crate::bytes;

/// Размер заголовка записи TLV.
pub const TLV_HEADER_SIZE: usize = 8;
/// Глубина вложенности, дальше которой payload считается «листом».
pub const TLV_MAX_DEPTH: usize = 3;

/// Запись TLV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tlv {
    /// Идентификатор поля.
    pub id: u32,
    /// Смещение записи относительно начала разбираемого буфера (с учётом `base`).
    pub offset: usize,
    /// Размер payload.
    pub size: u32,
    /// Содержимое без заголовка.
    pub payload: Vec<u8>,
    /// Вложенные записи, если payload сам оказался корректным TLV.
    pub children: Vec<Tlv>,
}

/// Разобрать поток TLV. `depth` — текущая вложенность, `base` — смещение
/// разбираемого буфера в родителе (для читаемых смещений).
pub fn parse_tlv(blob: &[u8], depth: usize, base: usize) -> Vec<Tlv> {
    let mut entries = Vec::new();
    let limit = blob.len();
    let mut offset = 0usize;
    while offset + TLV_HEADER_SIZE <= limit {
        let (Ok(id), Ok(size)) = (bytes::u32_at(blob, offset), bytes::u32_at(blob, offset + 4))
        else {
            break;
        };
        let Some(end) = offset.checked_add(TLV_HEADER_SIZE + size as usize) else {
            break;
        };
        if end > limit {
            break;
        }
        let payload = &blob[offset + TLV_HEADER_SIZE..end];
        let children = if depth < TLV_MAX_DEPTH && size >= 8 {
            nested(payload, depth + 1).unwrap_or_default()
        } else {
            Vec::new()
        };
        entries.push(Tlv {
            id,
            offset: base + offset,
            size,
            payload: payload.to_vec(),
            children,
        });
        offset = end;
    }
    entries
}

/// Разобрать payload как TLV: принимается только цепочка, покрывающая буфер целиком.
fn nested(blob: &[u8], depth: usize) -> Option<Vec<Tlv>> {
    let entries = parse_tlv(blob, depth, 0);
    let last = entries.last()?;
    if last.offset + TLV_HEADER_SIZE + last.size as usize == blob.len() {
        Some(entries)
    } else {
        None
    }
}
