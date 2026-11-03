//! Ошибки слоя формата. Сообщения — по-русски, варианты — по смыслу проверки.

use std::fmt;

use crate::{header, metadata, point, tile};

/// Результат операций над контейнером.
pub type Result<T> = std::result::Result<T, Error>;

/// Файл не соответствует ожидаемому контейнеру ADOCT.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Буфер короче, чем нужно для структуры.
    TooShort { size: usize, need: usize },
    /// Первые 8 байт не `ADOCT\0\0\0`.
    BadSignature([u8; 8]),
    /// В заголовке нет строки GUID в окне поиска.
    GuidMissing,
    /// GUID не закрыт `}`.
    GuidUnterminated,
    /// GUID не ASCII.
    GuidNotAscii,
    /// GUID не в формате `{8_4_4_4_12}`.
    GuidFormat(String),
    /// Длина строки GUID не 38 байт.
    GuidLength(usize),
    /// GUID найден не на `0xE3`: раскладка заголовка другая.
    GuidOffset(usize),
    /// Ни один кандидат директории не покрывает файл целиком.
    DirectoryNotFound,
    /// Список блоков пуст (писать нечего).
    NoBlocks,
    /// Количество блоков не совпало с контейнером.
    BlockCount { got: usize, expected: usize },
    /// Директория не помещается до первого блока.
    DirectoryOverflow { dir_end: usize, first_block: usize },
    /// Блок `type 3` не вмещает заявленное число записей.
    TileOverflow { count: u64, block_size: u64 },
    /// Запись узла собрана не того размера (не 368 байт).
    TileRecord { index: usize },
    /// Нет блока `type 11`, хотя узлы есть.
    NoPointsBlock,
    /// Не удалось подобрать раскладку payload'а узла по bbox.
    PayloadLayout { tile: usize, size: u64, count: u32 },
    /// Тело точки не 16 байт.
    BodySize(usize),
    /// Координата точки вне куба узла: квантование не помещается в `u16`.
    PointOutOfCube { axis: usize, value: f64 },
    /// Слов в заголовке метаданных больше 32.
    MetadataWords(usize),
    /// Число записей `type 1` не равно сумме слов заголовка.
    MetadataRecords { records: usize, sum: u32 },
    /// Размеры буферов различаются при сравнении.
    SizeMismatch { left: usize, right: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort { size, need } => {
                write!(f, "буфер {size} Б короче, нужно {need} Б")
            }
            Error::BadSignature(got) => {
                write!(f, "неверная сигнатура: {got:02x?}, ожидалось b\"ADOCT\\0\\0\\0\"")
            }
            Error::GuidMissing => write!(f, "не найден GUID '{{...}}' в заголовке"),
            Error::GuidUnterminated => write!(f, "GUID не закрыт символом '}}'"),
            Error::GuidNotAscii => write!(f, "GUID не является ASCII"),
            Error::GuidFormat(text) => {
                write!(f, "GUID должен быть в формате {{8_4_4_4_12}}, получено {text:?}")
            }
            Error::GuidLength(len) => write!(f, "GUID должен быть 38 байт, получено {len}"),
            Error::GuidOffset(offset) => write!(
                f,
                "GUID на {offset:#x}, ожидалось {:#x}: раскладка заголовка другая",
                header::GUID_OFFSET
            ),
            Error::DirectoryNotFound => write!(
                f,
                "директория блоков не найдена: ни один кандидат не покрывает файл целиком"
            ),
            Error::NoBlocks => write!(f, "нечего писать: список блоков пуст"),
            Error::BlockCount { got, expected } => {
                write!(f, "получено {got} блоков, в контейнере {expected}")
            }
            Error::DirectoryOverflow {
                dir_end,
                first_block,
            } => write!(
                f,
                "директория ({dir_end} Б) не помещается до первого блока ({first_block})"
            ),
            Error::TileOverflow { count, block_size } => write!(
                f,
                "блок type 3: {count} × {TILE_RECORD_SIZE} не помещается в {block_size} байт",
                TILE_RECORD_SIZE = tile::TILE_RECORD_SIZE
            ),
            Error::TileRecord { index } => {
                write!(f, "узел {index}: запись не {} Б", tile::TILE_RECORD_SIZE)
            }
            Error::NoPointsBlock => write!(f, "нет блока type 11"),
            Error::PayloadLayout { tile, size, count } => write!(
                f,
                "узел {tile}: не удалось подобрать раскладку payload ({size} Б, {count} точек)"
            ),
            Error::BodySize(size) => write!(
                f,
                "тело точки должно быть {} байт, получено {size}",
                point::BODY_SIZE
            ),
            Error::PointOutOfCube { axis, value } => write!(
                f,
                "координата по оси {axis} ({value}) не помещается в куб узла"
            ),
            Error::MetadataWords(count) => write!(
                f,
                "в заголовке метаданных {} слова, передано {count}",
                metadata::METADATA_WORD_COUNT
            ),
            Error::MetadataRecords { records, sum } => write!(
                f,
                "записей {records}, а сумма слов заголовка {sum}: в эталонах K = sum(words)"
            ),
            Error::SizeMismatch { left, right } => {
                write!(f, "размеры различаются: {left} против {right}")
            }
        }
    }
}

impl std::error::Error for Error {}
