//! Заголовок файла (`0x000 … 0x135`).
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §2): `0x10` — центр bbox (а не первая точка),
//! `0x08` — не число точек, `count` лежит на `0xD8`, `blockCount` — на `0x129`.
//! ГИПОТЕЗА: смысл `word4/word8/word12`, `chunk_id`, `flags` не расшифрован,
//! но они переносятся как есть, поэтому round-trip побайтовый.

use crate::bytes;
use crate::error::{Error, Result};
use crate::MAGIC;

/// Начало строки GUID в эталонах.
pub const GUID_OFFSET: usize = 0xE3;
/// Длина строки GUID `{8_4_4_4_12}`.
pub const GUID_LEN: usize = 38;
/// Начало директории блоков (заголовок кончается здесь).
pub const HEADER_END: usize = 0x135;
/// Смещение `blockCount` (u32).
pub const BLOCK_COUNT_OFFSET: usize = 0x129;
/// `u32` на `0x12D`: во всех эталонах `2`.
pub const HEADER_WORD_0X12D: u32 = 2;
/// Окно поиска GUID после `0xE0`.
const GUID_SEARCH_WINDOW: usize = 0x80;
/// Минимальный размер буфера, при котором заголовок вообще читается.
const MIN_HEADER_BYTES: usize = 0x120;

/// Разобранный заголовок файла.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// `0x04`: во всех эталонах `84`.
    pub word4: u32,
    /// `0x08`: во всех эталонах `3` (НЕ число точек).
    pub word8: u32,
    /// `0x0C`: во всех эталонах `2`.
    pub word12: u32,
    /// `0x10` — центр bbox.
    pub center: [f64; 3],
    /// `0x28` — нули.
    pub zeros28: [f64; 3],
    /// `0x40` — единицы.
    pub unit: [f64; 3],
    /// `0x58` — повтор центра.
    pub center2: [f64; 3],
    /// `0x70` — `-R` корневого куба.
    pub root_min: [f64; 3],
    /// `0x88` — `+R` корневого куба.
    pub root_max: [f64; 3],
    /// `0xA0` — `-(hx, hy, hz)`.
    pub extent_min: [f64; 3],
    /// `0xB8` — `+(hx, hy, hz)`.
    pub extent_max: [f64; 3],
    /// `0xD0`.
    pub chunk_id: u32,
    /// `0xD4` = 5 в экспорте, 2/1/134 в raw.
    pub chunk_version: u32,
    /// `0xD8` — число точек (подтверждено на эталонах).
    pub count: u64,
    /// `0xE0..0xE3` = `01 01 00`.
    pub flags: [u8; 3],
    /// Строка GUID вместе с фигурными скобками.
    pub guid: String,
    /// Фактическое смещение строки GUID.
    pub guid_offset: usize,
}

impl Header {
    /// Конец строки GUID.
    pub fn guid_end(&self) -> usize {
        self.guid_offset + self.guid.len()
    }

    /// Разобрать заголовок из начала файла.
    pub fn parse(data: &[u8]) -> Result<Header> {
        if data.len() < MIN_HEADER_BYTES {
            return Err(Error::TooShort { size: data.len(), need: MIN_HEADER_BYTES });
        }
        if data[..8] != MAGIC {
            let mut got = [0u8; 8];
            got.copy_from_slice(&data[..8]);
            return Err(Error::BadSignature(got));
        }

        let (guid, guid_offset) = read_guid(data, 0xE0)?;
        Ok(Header {
            word4: bytes::u32_at(data, 0x04)?,
            word8: bytes::u32_at(data, 0x08)?,
            word12: bytes::u32_at(data, 0x0C)?,
            center: bytes::f64x3_at(data, 0x10)?,
            zeros28: bytes::f64x3_at(data, 0x28)?,
            unit: bytes::f64x3_at(data, 0x40)?,
            center2: bytes::f64x3_at(data, 0x58)?,
            root_min: bytes::f64x3_at(data, 0x70)?,
            root_max: bytes::f64x3_at(data, 0x88)?,
            extent_min: bytes::f64x3_at(data, 0xA0)?,
            extent_max: bytes::f64x3_at(data, 0xB8)?,
            chunk_id: bytes::u32_at(data, 0xD0)?,
            chunk_version: bytes::u32_at(data, 0xD4)?,
            count: bytes::u64_at(data, 0xD8)?,
            flags: [data[0xE0], data[0xE1], data[0xE2]],
            guid,
            guid_offset,
        })
    }

    /// Собрать первые `HEADER_END` байт файла.
    ///
    /// `guid` — строка GUID (`None` — сохранить исходную), `block_count` — число
    /// записей директории.
    pub fn to_bytes(&self, guid: Option<&str>, block_count: usize) -> Result<Vec<u8>> {
        if self.guid_offset != GUID_OFFSET {
            return Err(Error::GuidOffset(self.guid_offset));
        }
        let guid = match guid {
            Some(text) => normalize_guid(text)?,
            None => self.guid.clone(),
        };
        if guid.len() != GUID_LEN {
            return Err(Error::GuidLength(guid.len()));
        }

        let mut out = vec![0u8; HEADER_END];
        out[..8].copy_from_slice(&MAGIC);
        bytes::put_u32(&mut out, 0x04, self.word4);
        bytes::put_u32(&mut out, 0x08, self.word8);
        bytes::put_u32(&mut out, 0x0C, self.word12);
        bytes::put_f64x3(&mut out, 0x10, self.center);
        bytes::put_f64x3(&mut out, 0x28, self.zeros28);
        bytes::put_f64x3(&mut out, 0x40, self.unit);
        bytes::put_f64x3(&mut out, 0x58, self.center2);
        bytes::put_f64x3(&mut out, 0x70, self.root_min);
        bytes::put_f64x3(&mut out, 0x88, self.root_max);
        bytes::put_f64x3(&mut out, 0xA0, self.extent_min);
        bytes::put_f64x3(&mut out, 0xB8, self.extent_max);
        bytes::put_u32(&mut out, 0xD0, self.chunk_id);
        bytes::put_u32(&mut out, 0xD4, self.chunk_version);
        bytes::put_u64(&mut out, 0xD8, self.count);
        out[0xE0..0xE3].copy_from_slice(&self.flags);
        out[GUID_OFFSET..GUID_OFFSET + GUID_LEN].copy_from_slice(guid.as_bytes());
        bytes::put_u32(&mut out, BLOCK_COUNT_OFFSET, block_count as u32);
        bytes::put_u32(&mut out, BLOCK_COUNT_OFFSET + 4, HEADER_WORD_0X12D);
        Ok(out)
    }
}

/// Найти строку GUID: она хранится без префикса длины, в окне после `0xE0`.
fn read_guid(data: &[u8], search_from: usize) -> Result<(String, usize)> {
    let limit = data.len().min(search_from + GUID_SEARCH_WINDOW);
    let start = data
        .get(search_from..limit)
        .and_then(|window| window.iter().position(|byte| *byte == b'{'))
        .map(|pos| search_from + pos)
        .ok_or(Error::GuidMissing)?;
    let end = data[start..limit]
        .iter()
        .position(|byte| *byte == b'}')
        .map(|pos| start + pos)
        .ok_or(Error::GuidUnterminated)?;
    let text = std::str::from_utf8(&data[start..=end]).map_err(|_| Error::GuidNotAscii)?;
    Ok((text.to_string(), start))
}

/// Проверить GUID и привести к виду ReCap: подчёркивания, нижний регистр.
///
/// Принимаются и дефисы (привычный вид uuid4), на выходе всегда подчёркивания.
pub fn normalize_guid(text: &str) -> Result<String> {
    let value = text.trim();
    let body = value
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| Error::GuidFormat(text.to_string()))?;
    let groups: Vec<&str> = body.split(['-', '_']).collect();
    let ok = groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && body
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-' || byte == b'_');
    if !ok {
        return Err(Error::GuidFormat(text.to_string()));
    }
    Ok(format!("{{{}}}", body.replace('-', "_").to_ascii_lowercase()))
}
