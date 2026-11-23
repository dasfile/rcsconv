//! bbox and the point-source contract.

use crate::error::Result;
use crate::point::CloudPoint;
use crate::progress::Progress;

/// bbox and point count of a source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// bbox minimum.
    pub low: [f64; 3],
    /// bbox maximum.
    pub high: [f64; 3],
    /// Number of points.
    pub count: u64,
}

impl Bounds {
    /// bbox centre — becomes the header `center`.
    pub fn center(&self) -> [f64; 3] {
        [
            (self.low[0] + self.high[0]) / 2.0,
            (self.low[1] + self.high[1]) / 2.0,
            (self.low[2] + self.high[2]) / 2.0,
        ]
    }

    /// bbox half-extent — becomes the header `extent`.
    pub fn half_extent(&self) -> [f64; 3] {
        [
            (self.high[0] - self.low[0]) / 2.0,
            (self.high[1] - self.low[1]) / 2.0,
            (self.high[2] - self.low[2]) / 2.0,
        ]
    }
}

/// Point source for building a `.rcs`.
///
/// The contract mirrors `cloud.CloudSource`: first `bounds()`, then `points()`.
/// In Python `points()` is a multi-pass generator; in Rust points are read into
/// a single vector (about 128 MB for 4 M points) so there is one pass and the
/// order is guaranteed — important for byte parity.
pub trait Source {
    /// bbox and point count.
    fn bounds(&self) -> Result<Bounds>;

    /// bbox and count when the source knows them without reading every point
    /// (LAS/LAZ take them from the header; `.pts` returns `None`).
    fn bounds_hint(&self) -> Option<Bounds> {
        None
    }

    /// Stream the points in file order without keeping them in memory.
    ///
    /// The streaming writer calls this two or three times (counts, partition);
    /// implementations must be able to restart from the beginning.
    fn scan(
        &mut self,
        progress: &dyn Progress,
        visit: &mut dyn FnMut(CloudPoint) -> Result<()>,
    ) -> Result<()>;

    /// Same as [`Source::bounds`], but reports read progress.
    ///
    /// Sources that must read the whole file to get the bbox override this
    /// method; by default no progress is reported.
    fn bounds_with(&self, progress: &dyn Progress) -> Result<Bounds> {
        let _ = progress;
        self.bounds()
    }

    /// All points in file order (takes the vector out of the source).
    fn points(&mut self) -> Result<Vec<CloudPoint>>;

    /// Same as [`Source::points`], but reports progress.
    ///
    /// By default no progress is reported — important for parity tests:
    /// interfering with point order or content is not allowed.
    fn points_with(&mut self, progress: &dyn Progress) -> Result<Vec<CloudPoint>> {
        let _ = progress;
        self.points()
    }

    /// What this source is (for the CLI report).
    fn describe(&self) -> String;
}
