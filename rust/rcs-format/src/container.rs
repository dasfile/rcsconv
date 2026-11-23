//! Container: header + block directory + access to contents.

use crate::bytes;
use crate::directory::{self, Block, Dialect, DIR_RECORD_SIZE};
use crate::error::Result;
use crate::header::Header;
use crate::tlv::{self, Tlv};

/// Parsed `.rcs` on top of a slice of the original bytes.
///
/// Bytes are not copied: `Container` borrows the slice, so it is cheap for large
/// files (`level_7.rcs` is 7 MB, real clouds are tens of MB).
#[derive(Debug)]
pub struct Container<'a> {
    raw: &'a [u8],
    header: Header,
    directory_offset: usize,
    blocks: Vec<Block>,
    blocks_trailing: usize,
}

impl<'a> Container<'a> {
    /// Parse the container from the file bytes.
    pub fn parse(raw: &'a [u8]) -> Result<Container<'a>> {
        let header = Header::parse(raw)?;
        let (directory_offset, blocks) = directory::find_directory(raw, &header)?;
        let consumed = directory_offset + DIR_RECORD_SIZE * blocks.len();
        let trailing = (blocks[0].offset as usize).saturating_sub(consumed);
        Ok(Container { raw, header, directory_offset, blocks, blocks_trailing: trailing })
    }

    /// Original file bytes.
    pub fn raw(&self) -> &'a [u8] {
        self.raw
    }

    /// File size.
    pub fn size(&self) -> usize {
        self.raw.len()
    }

    /// Header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Offset of the block directory.
    pub fn directory_offset(&self) -> usize {
        self.directory_offset
    }

    /// Directory records in file order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// NUL terminator of the directory and the gap before the first block.
    pub fn blocks_trailing(&self) -> usize {
        self.blocks_trailing
    }

    /// First block of the given type.
    pub fn block_of_type(&self, kind: u64) -> Option<Block> {
        self.blocks.iter().copied().find(|block| block.kind == kind)
    }

    /// Block types in file order.
    pub fn block_types(&self) -> Vec<u64> {
        self.blocks.iter().map(|block| block.kind).collect()
    }

    /// Block contents (a slice of the original bytes).
    pub fn read_block(&self, block: Block) -> &'a [u8] {
        &self.raw[block.offset as usize..block.end() as usize]
    }

    /// Dialect: by `chunk_version` first, otherwise by the block layout.
    pub fn dialect(&self) -> Dialect {
        match directory::dialect_of(self.header.chunk_version) {
            Dialect::Unknown => directory::dialect_of_blocks(&self.block_types()),
            found => found,
        }
    }

    /// Number of records in the `type 3` block (`None` when the block is missing).
    pub fn tile_count(&self) -> Option<u64> {
        let block = self.block_of_type(3)?;
        if block.size < 8 {
            return None;
        }
        bytes::u64_at(self.raw, block.offset as usize).ok()
    }

    /// Tile record size from the invariant "8 + count × stride = block size".
    pub fn tile_record_size(&self) -> Option<usize> {
        let block = self.block_of_type(3)?;
        let count = self.tile_count()?;
        if count == 0 {
            return None;
        }
        let payload = block.size.checked_sub(8)?;
        if payload % count != 0 {
            return None;
        }
        Some((payload / count) as usize)
    }

    /// Bytes per point (for reports).
    pub fn bytes_per_point(&self) -> Option<f64> {
        let block = self.block_of_type(11)?;
        let count = self.header.count;
        if count == 0 {
            return None;
        }
        Some(block.size as f64 / count as f64)
    }

    /// Properties block: `type 5` (raw) or `type 0` when it is not a JPEG preview.
    pub fn tlv_block(&self) -> Option<Block> {
        if let Some(block) = self.block_of_type(5) {
            return Some(block);
        }
        let block = self.block_of_type(0)?;
        if directory::is_jpeg(self.read_block(block)) {
            None
        } else {
            Some(block)
        }
    }

    /// Preview block: `type 0` when it holds a JPEG.
    pub fn preview_block(&self) -> Option<Block> {
        let block = self.block_of_type(0)?;
        if directory::is_jpeg(self.read_block(block)) {
            Some(block)
        } else {
            None
        }
    }

    /// TLV records of the properties block.
    pub fn block_tlvs(&self, block: Block) -> Vec<Tlv> {
        tlv::parse_tlv(self.read_block(block), 0, 0)
    }
}
