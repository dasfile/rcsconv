//! Контейнер: заголовок + директория блоков + доступ к содержимому.

use crate::bytes;
use crate::directory::{self, Block, Dialect, DIR_RECORD_SIZE};
use crate::error::Result;
use crate::header::Header;
use crate::tlv::{self, Tlv};

/// Разобранный `.rcs` поверх среза с исходными байтами.
///
/// Байты не копируются: `Container` занимает срез, поэтому дешёв для больших
/// файлов (`level_7.rcs` — 7 МБ, облака — десятки МБ).
#[derive(Debug)]
pub struct Container<'a> {
    raw: &'a [u8],
    header: Header,
    directory_offset: usize,
    blocks: Vec<Block>,
    blocks_trailing: usize,
}

impl<'a> Container<'a> {
    /// Разобрать контейнер из байтов файла.
    pub fn parse(raw: &'a [u8]) -> Result<Container<'a>> {
        let header = Header::parse(raw)?;
        let (directory_offset, blocks) = directory::find_directory(raw, &header)?;
        let consumed = directory_offset + DIR_RECORD_SIZE * blocks.len();
        let trailing = (blocks[0].offset as usize).saturating_sub(consumed);
        Ok(Container { raw, header, directory_offset, blocks, blocks_trailing: trailing })
    }

    /// Исходные байты файла.
    pub fn raw(&self) -> &'a [u8] {
        self.raw
    }

    /// Размер файла.
    pub fn size(&self) -> usize {
        self.raw.len()
    }

    /// Заголовок.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Смещение директории блоков.
    pub fn directory_offset(&self) -> usize {
        self.directory_offset
    }

    /// Записи директории в порядке файла.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Нули-терминатор директории и резерв до первого блока.
    pub fn blocks_trailing(&self) -> usize {
        self.blocks_trailing
    }

    /// Первый блок нужного типа.
    pub fn block_of_type(&self, kind: u64) -> Option<Block> {
        self.blocks.iter().copied().find(|block| block.kind == kind)
    }

    /// Типы блоков в порядке файла.
    pub fn block_types(&self) -> Vec<u64> {
        self.blocks.iter().map(|block| block.kind).collect()
    }

    /// Содержимое блока (срез исходных байтов).
    pub fn read_block(&self, block: Block) -> &'a [u8] {
        &self.raw[block.offset as usize..block.end() as usize]
    }

    /// Диалект: сначала по `chunk_version`, иначе по составу блоков.
    pub fn dialect(&self) -> Dialect {
        match directory::dialect_of(self.header.chunk_version) {
            Dialect::Unknown => directory::dialect_of_blocks(&self.block_types()),
            found => found,
        }
    }

    /// Число записей в блоке `type 3` (`None`, если блока нет).
    pub fn tile_count(&self) -> Option<u64> {
        let block = self.block_of_type(3)?;
        if block.size < 8 {
            return None;
        }
        bytes::u64_at(self.raw, block.offset as usize).ok()
    }

    /// Размер записи узла по инварианту «8 + count × stride = size блока».
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

    /// Байт на точку (для отчётов).
    pub fn bytes_per_point(&self) -> Option<f64> {
        let block = self.block_of_type(11)?;
        let count = self.header.count;
        if count == 0 {
            return None;
        }
        Some(block.size as f64 / count as f64)
    }

    /// Блок со свойствами: `type 5` (raw) или `type 0`, если это не JPEG-превью.
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

    /// Блок превью: `type 0`, если внутри JPEG.
    pub fn preview_block(&self) -> Option<Block> {
        let block = self.block_of_type(0)?;
        if directory::is_jpeg(self.read_block(block)) {
            Some(block)
        } else {
            None
        }
    }

    /// TLV-записи блока свойств.
    pub fn block_tlvs(&self, block: Block) -> Vec<Tlv> {
        tlv::parse_tlv(self.read_block(block), 0, 0)
    }
}
