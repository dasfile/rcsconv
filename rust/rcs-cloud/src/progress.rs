//! Progress and cancellation for long operations.
//!
//! Needed by the GUI (`rcsconv-gui`): a per-stage progress bar and a Cancel button.
//! The CLI passes [`NoProgress`], so neither the output nor the reference bytes change.

/// `.rcs` build stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading the source (`.las/.laz/.pts`).
    Read,
    /// Choosing the depth and splitting points into cells.
    Octree,
    /// Writing tiles (`type 3`) and point bodies (`type 11`).
    Tiles,
    /// Assembling the container: header, directory, blocks.
    Write,
}

impl Stage {
    /// Stage title for the interface.
    pub fn title(self) -> &'static str {
        match self {
            Stage::Read => "reading",
            Stage::Octree => "octree",
            Stage::Tiles => "tiles",
            Stage::Write => "writing",
        }
    }
}

/// Receiver of build progress reports.
///
/// Implementations must be cheap: `stage` is called often (a few times per
/// million points), and `cancelled` even more often, at every check.
pub trait Progress: Send + Sync {
    /// Current stage: `done` out of `total` (`total = 0` means unknown).
    fn stage(&self, stage: Stage, done: u64, total: u64) {
        let _ = (stage, done, total);
    }

    /// Whether cancellation was requested. Checked at build stop points.
    fn cancelled(&self) -> bool {
        false
    }
}

/// No-op receiver: no reports, no cancellation (CLI, tests, parity runs).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoProgress;

impl Progress for NoProgress {}
