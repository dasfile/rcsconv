//! Cloud layer errors.

use std::fmt;

/// Result of build operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The cloud could not be read or split into tiles.
#[derive(Debug)]
pub enum Error {
    /// Input/output error.
    Io(std::io::Error),
    /// Format layer error.
    Format(rcs_format::Error),
    /// The source contains no points.
    EmptyCloud,
    /// Points per tile limit is outside the allowed range.
    MaxPerTile(u32),
    /// Points cannot be split into cells: duplicates at one position.
    DepthExceeded,
    /// A coordinate does not fit into `u16` during quantisation.
    PointOutOfCube { axis: usize, value: f64 },
    /// Source read error (input format).
    Source(String),
    /// The user cancelled the operation (checked via [`crate::progress::Progress`]).
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O: {err}"),
            Error::Format(err) => write!(f, ".rcs format: {err}"),
            Error::EmptyCloud => write!(f, "the source contains no points"),
            Error::MaxPerTile(value) => write!(
                f,
                "max_points_per_tile={value}: allowed 1..{} (a tile cannot hold \
                 more bodies than the u16 index in the payload header)",
                crate::MAX_TILE_POINTS
            ),
            Error::DepthExceeded => {
                write!(f, "cannot split points into cells: duplicate points")
            }
            Error::PointOutOfCube { axis, value } => write!(
                f,
                "coordinate on axis {axis} ({value}) does not fit into u16"
            ),
            Error::Source(text) => write!(f, "{text}"),
            Error::Cancelled => write!(f, "cancelled by user"),
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
