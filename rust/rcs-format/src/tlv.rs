//! TLV stream of the properties block (`type 0` in exports, `type 5` in raw).
//!
//! CONFIRMED (`docs/RCS-FORMAT.md` 3): a chain of `(id u32, size u32, payload)`;
//! `id = 10` holds nested TLV, other ids are scalars. Parsing is tolerant: the
//! stream is read until the first mismatch, as in `tools/rcs_blocks.py`.

use crate::bytes;

/// Size of the TLV record header.
pub const TLV_HEADER_SIZE: usize = 8;
/// Nesting depth beyond which the payload is treated as a leaf.
pub const TLV_MAX_DEPTH: usize = 3;

/// A TLV record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tlv {
    /// Field identifier.
    pub id: u32,
    /// Record offset relative to the start of the parsed buffer (including `base`).
    pub offset: usize,
    /// Payload size.
    pub size: u32,
    /// Contents without the header.
    pub payload: Vec<u8>,
    /// Nested records when the payload itself is a valid TLV stream.
    pub children: Vec<Tlv>,
}

/// Parse a TLV stream. `depth` is the current nesting, `base` is the offset of
/// the parsed buffer inside its parent (for readable offsets).
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

/// Parse the payload as TLV: accepted only if the chain covers the whole buffer.
fn nested(blob: &[u8], depth: usize) -> Option<Vec<Tlv>> {
    let entries = parse_tlv(blob, depth, 0);
    let last = entries.last()?;
    if last.offset + TLV_HEADER_SIZE + last.size as usize == blob.len() {
        Some(entries)
    } else {
        None
    }
}
