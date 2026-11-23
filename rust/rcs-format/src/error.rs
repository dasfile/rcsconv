//! Format layer errors. Messages are English, variants follow the check semantics.

use std::fmt;

use crate::{header, metadata, point, tile};

/// Result of container operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The file does not match the expected ADOCT container.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The buffer is shorter than the structure requires.
    TooShort { size: usize, need: usize },
    /// The first 8 bytes are not `ADOCT\0\0\0`.
    BadSignature([u8; 8]),
    /// The header has no GUID string inside the search window.
    GuidMissing,
    /// The GUID is not terminated by `}`.
    GuidUnterminated,
    /// The GUID is not ASCII.
    GuidNotAscii,
    /// The GUID is not in the `{8_4_4_4_12}` format.
    GuidFormat(String),
    /// The GUID string is not 38 bytes long.
    GuidLength(usize),
    /// The GUID is not at `0xE3`: the header layout differs.
    GuidOffset(usize),
    /// No directory candidate covers the whole file.
    DirectoryNotFound,
    /// The block list is empty (nothing to write).
    NoBlocks,
    /// The block count does not match the container.
    BlockCount { got: usize, expected: usize },
    /// The directory does not fit before the first block.
    DirectoryOverflow { dir_end: usize, first_block: usize },
    /// The `type 3` block cannot hold the declared number of records.
    TileOverflow { count: u64, block_size: u64 },
    /// A tile record was built with the wrong size (not 368 bytes).
    TileRecord { index: usize },
    /// There is no `type 11` block although tiles exist.
    NoPointsBlock,
    /// Could not fit the tile payload layout by bbox.
    PayloadLayout { tile: usize, size: u64, count: u32 },
    /// The point body is not 16 bytes.
    BodySize(usize),
    /// A point coordinate is outside the tile cube: quantisation does not fit into `u16`.
    PointOutOfCube { axis: usize, value: f64 },
    /// The metadata header has more than 32 words.
    MetadataWords(usize),
    /// The number of `type 1` records differs from the header word sum.
    MetadataRecords { records: usize, sum: u32 },
    /// Buffer sizes differ when comparing.
    SizeMismatch { left: usize, right: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort { size, need } => {
                write!(f, "buffer is {size} B, {need} B required")
            }
            Error::BadSignature(got) => {
                write!(f, "bad signature: {got:02x?}, expected b\"ADOCT\\0\\0\\0\"")
            }
            Error::GuidMissing => write!(f, "GUID '{{...}}' not found in the header"),
            Error::GuidUnterminated => write!(f, "GUID is not terminated by '}}'"),
            Error::GuidNotAscii => write!(f, "GUID is not ASCII"),
            Error::GuidFormat(text) => {
                write!(f, "GUID must use the {{8_4_4_4_12}} layout, got {text:?}")
            }
            Error::GuidLength(len) => write!(f, "GUID must be 38 bytes, got {len}"),
            Error::GuidOffset(offset) => write!(
                f,
                "GUID at {offset:#x}, expected {:#x}: header layout differs",
                header::GUID_OFFSET
            ),
            Error::DirectoryNotFound => write!(
                f,
                "block directory not found: no candidate covers the whole file"
            ),
            Error::NoBlocks => write!(f, "nothing to write: the block list is empty"),
            Error::BlockCount { got, expected } => {
                write!(f, "got {got} blocks, the container has {expected}")
            }
            Error::DirectoryOverflow {
                dir_end,
                first_block,
            } => write!(
                f,
                "directory ({dir_end} B) does not fit before the first block ({first_block})"
            ),
            Error::TileOverflow { count, block_size } => write!(
                f,
                "type 3 block: {count} × {TILE_RECORD_SIZE} does not fit into {block_size} bytes",
                TILE_RECORD_SIZE = tile::TILE_RECORD_SIZE
            ),
            Error::TileRecord { index } => {
                write!(f, "tile {index}: record is not {} B", tile::TILE_RECORD_SIZE)
            }
            Error::NoPointsBlock => write!(f, "no type 11 block"),
            Error::PayloadLayout { tile, size, count } => write!(
                f,
                "tile {tile}: cannot fit the payload layout ({size} B, {count} points)"
            ),
            Error::BodySize(size) => write!(
                f,
                "point body must be {} bytes, got {size}",
                point::BODY_SIZE
            ),
            Error::PointOutOfCube { axis, value } => write!(
                f,
                "coordinate on axis {axis} ({value}) does not fit into the tile cube"
            ),
            Error::MetadataWords(count) => write!(
                f,
                "metadata header has {} words, got {count}",
                metadata::METADATA_WORD_COUNT
            ),
            Error::MetadataRecords { records, sum } => write!(
                f,
                "{records} records but the header word sum is {sum}: in the reference corpus K = sum(words)"
            ),
            Error::SizeMismatch { left, right } => {
                write!(f, "sizes differ: {left} vs {right}")
            }
        }
    }
}

impl std::error::Error for Error {}
