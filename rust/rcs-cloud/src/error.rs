//! Ошибки слоя облака.

use std::fmt;

/// Результат операций сборки.
pub type Result<T> = std::result::Result<T, Error>;

/// Облако не удалось прочитать или разложить по узлам.
#[derive(Debug)]
pub enum Error {
    /// Ошибка ввода-вывода.
    Io(std::io::Error),
    /// Ошибка слоя формата.
    Format(rcs_format::Error),
    /// В источнике нет точек.
    EmptyCloud,
    /// Предел точек в узле вне допустимого диапазона.
    MaxPerTile(u32),
    /// Точки не разводятся по ячейкам: дубликаты в одной точке.
    DepthExceeded,
    /// Координата не помещается в `u16` при квантовании.
    PointOutOfCube { axis: usize, value: f64 },
    /// Ошибка чтения источника (формат входа).
    Source(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "ввод-вывод: {err}"),
            Error::Format(err) => write!(f, "формат .rcs: {err}"),
            Error::EmptyCloud => write!(f, "в источнике нет точек"),
            Error::MaxPerTile(value) => write!(
                f,
                "max_points_per_tile={value}: допустимо 1…{} (в узле не может быть \
                 больше тел, чем вмещает u16 index)",
                crate::MAX_TILE_POINTS
            ),
            Error::DepthExceeded => {
                write!(f, "не удалось развести точки по ячейкам — есть дубликаты")
            }
            Error::PointOutOfCube { axis, value } => write!(
                f,
                "координата по оси {axis} ({value}) не помещается в u16"
            ),
            Error::Source(text) => write!(f, "{text}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<rcs_format::Error> for Error {
    fn from(err: rcs_format::Error) -> Self {
        Error::Format(err)
    }
}
