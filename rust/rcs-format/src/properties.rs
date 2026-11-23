//! Properties block (`type 0` in exports, `type 5` in raw) — TLV.
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 3, 4.4): in exports the block is byte-identical
//! for `probe6/test1/test2/level_7` and is 134 bytes:
//! `u32 10` (attribute schema version) + TLV chain
//! `100` — u8 version, `101/102` — f32 bounds of the active attribute (stubs
//! -999999/+999999), `201/202` — u32 0, `203..206` — f64 ±π, ±π/2, `301` — u8 flag.

use crate::bytes;
use crate::tlv::{parse_tlv, Tlv};

/// Leading `u32` of the properties block.
pub const PROPS_VERSION: u32 = 10;
/// "Upper bound" stub of the active attribute.
pub const PROPS_UPPER: f32 = -999_999.0;
/// "Lower bound" stub of the active attribute.
pub const PROPS_LOWER: f32 = 999_999.0;

/// Parsed properties block: schema version + TLV chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    /// Leading `u32`.
    pub version: u32,
    /// Fields after the version.
    pub entries: Vec<Tlv>,
}

impl Properties {
    /// Parse the properties block.
    pub fn parse(block: &[u8]) -> Properties {
        let version = bytes::u32_at(block, 0).unwrap_or(0);
        let body = block.get(4..).unwrap_or(&[]);
        Properties { version, entries: parse_tlv(body, 0, 4) }
    }

    /// Value of field `id` as `u8` (for example `100` — attribute schema version).
    pub fn u8_field(&self, id: u32) -> Option<u8> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        entry.payload.first().copied()
    }

    /// Value of field `id` as `f32` (for example `101`/`102` — attribute bounds).
    pub fn f32_field(&self, id: u32) -> Option<f32> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        let raw: [u8; 4] = entry.payload.get(..4)?.try_into().ok()?;
        Some(f32::from_le_bytes(raw))
    }

    /// Value of field `id` as `f64` (for example `203` — π).
    pub fn f64_field(&self, id: u32) -> Option<f64> {
        let entry = self.entries.iter().find(|entry| entry.id == id)?;
        let raw: [u8; 8] = entry.payload.get(..8)?.try_into().ok()?;
        Some(f64::from_le_bytes(raw))
    }
}

/// Build one TLV record.
pub fn tlv_entry(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Build the export properties block (134 bytes).
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

/// Properties block with the reference bound stubs.
pub fn build_default_properties_block() -> Vec<u8> {
    build_properties_block(PROPS_UPPER, PROPS_LOWER)
}
