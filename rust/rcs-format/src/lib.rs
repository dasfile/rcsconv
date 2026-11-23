//! Autodesk ReCap `.rcs` container (signature `ADOCT`) — the format layer.
//!
//! Port of `tools/rcs_blocks.py`, `tools/rcs_points.py`, `tools/rcs_write.py`
//! (Python is the reference; parity is checked byte by byte, `docs/ROADMAP.md` R1).
//!
//! Confirmed by the reference bytes (`docs/RCS-FORMAT.md`):
//!
//! * header `0x000..0x135` — signature, bbox centre, root cube, `chunk_version`,
//!   `count`, `flags`, GUID string `{8_4_4_4_12}` at `0xE3`, `blockCount` at `0x129`;
//! * block directory at `0x135` — records `(offset u64, size u64, type u64)`,
//!   blocks are contiguous and end exactly at EOF (no alignment);
//! * block `type 3` — `u64 count` + `count × 368` tile records;
//! * block `type 11` — tile payloads back to back, point body is 16 bytes (per-axis quantisation);
//! * block `type 1` — 128-byte header (32 × u32) + 72-byte records;
//! * block `type 0` (export) — TLV with the attribute schema version.
//!
//! The public API mirrors the Python one: `Container::parse` -> `iter_tiles` ->
//! `decode_tile`; building goes through `writer::serialize`.

#![forbid(unsafe_code)]

mod bytes;

pub mod container;
pub mod directory;
pub mod error;
pub mod header;
pub mod metadata;
pub mod num;
pub mod point;
pub mod properties;
pub mod tile;
pub mod tlv;
pub mod writer;

pub use container::Container;
pub use directory::{Block, FIRST_BLOCK_OFFSET};
pub use error::{Error, Result};
pub use header::Header;
pub use metadata::Metadata;
pub use point::Point;
pub use tile::Tile;
pub use tlv::Tlv;

/// Container signature: `ADOCT` plus three zero bytes.
pub const MAGIC: [u8; 8] = *b"ADOCT\0\0\0";
