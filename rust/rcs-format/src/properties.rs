//! Блок свойств (`type 0` в экспорте, `type 5` в raw) — TLV.
//!
//! ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §3, §4.4): в экспорте блок байт-в-байт
//! одинаков у `probe6/test1/test2/level_7` и равен 134 байтам:
//! `u32 10` (версия схемы атрибутов) + цепочка TLV
//! `100` — u8 версия, `101/102` — f32 границы активного атрибута (заглушки
//! ∓999999), `201/202` — u32 0, `203…206` — f64 ±π, ±π/2, `301` — u8 флаг.

use crate::bytes;
use crate::tlv::{parse_tlv, Tlv};

/// Ведущее `u32` блока свойств.
pub const PROPS_VERSION: u32 = 10;
/// Заглушка «верхней границы» активного атрибута.
pub const PROPS_UPPER: f32 = -999_999.0;
/// Заглушка «нижней границы» активного атрибута.
pub const PROPS_LOWER: f32 = 999_999.0;

/// Разобранный блок свойств: версия схемы + TLV-цепочка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    /// Ведущее `u32`.
    pub version: u32,
    /// Поля после версии.
    pub entries: Vec<Tlv>,
}

impl Properties {
    /// Разобрать блок свойств.
    pub fn parse(block: &[u8]) -> Properties {
        let version = bytes::u32_at(block, 0).unwrap_or(0);
        let body = block.get(4..).unwrap_or(&[]);
        Properties { version, entries: parse_tlv(body, 0, 4) }
    }

    /// Значение поля `id` как `u8` (например, `100` — версия схемы атрибутов).
    pub fn u8_field(&self, id: u32) -> Option<u8> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        entry.payload.first().copied()
    }

    /// Значение поля `id` как `f32` (например, `101`/`102` — границы атрибута).
    pub fn f32_field(&self, id: u32) -> Option<f32> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        let raw: [u8; 4] = entry.payload.get(..4)?.try_into().ok()?;
        Some(f32::from_le_bytes(raw))
    }

    /// Значение поля `id` как `f64` (например, `203` — π).
    pub fn f64_field(&self, id: u32) -> Option<f64> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        let raw: [u8; 8] = entry.payload.get(..8)?.try_into().ok()?;
        Some(f64::from_le_bytes(raw))
    }
}

/// Собрать одну запись TLV.
pub fn tlv_entry(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Собрать блок свойств экспорта (134 байта).
pub fn build_properties_block(upper: f32, lower: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(134);
    out.extend_from_slice(&PROPS_VERSION.to_le_bytes());
    out.extend(tlv_entry(100, &[0x01]));
    out.extend(tlv_entry(101, &upper.to_le_bytes()));
    out.extend(tlv_entry(102, &lower.to_le_bytes()));
    out.extend(tlv_entry(201, &0u32.to_le_bytes()));
    out.extend(tlv_entry(202, &0u32.to_le_bytes()));
    out.extend(tlv_entry(203, &std::f64::consts::PI.to_le_bytes()));
    out.extend(tlv_entry(204, &(-std::f64::consts::PI).to_le_bytes()));
    out.extend(tlv_entry(205, &(std::f64::consts::PI / 2.0).to_le_bytes()));
    out.extend(tlv_entry(206, &(-std::f64::consts::PI / 2.0).to_le_bytes()));
    out.extend(tlv_entry(301, &[0x00]));
    out
}

/// Блок свойств с эталонными заглушками границ.
pub fn build_default_properties_block() -> Vec<u8> {
    build_properties_block(PROPS_UPPER, PROPS_LOWER)
}
