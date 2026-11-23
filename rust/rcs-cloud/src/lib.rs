//! Point cloud layer: point, source, octree, `.rcs` build.
//!
//! Port of `tools/cloud.py` + `tools/rcs_build.py` (Python is the reference,
//! parity is checked byte by byte, `docs/ROADMAP.md` R2/R4).
//!
//! This module knows nothing about input formats: a point source implements
//! [`Source`], so `.pts` (`cloud-pts`) and `.las/.laz` (`cloud-las`) are plugged
//! in from outside. The result is Unified RCS (dialect v5, `chunk_version = 5`).

#![forbid(unsafe_code)]

pub mod build;
pub mod error;
pub mod guid;
pub mod octree;
pub mod point;
pub mod progress;
pub mod source;
pub mod stream;

pub use build::{build_from_points, build_from_points_with, build_from_source, build_from_source_with, BuildOptions, Stats};
pub use error::{Error, Result};
pub use point::CloudPoint;
pub use progress::{NoProgress, Progress, Stage};
pub use rcs_format::num::round_half_even;
pub use rcs_format::header::normalize_guid;
pub use source::{Bounds, Source};

/// Tile edge, metres: the same in every ReCap reference file.
pub const TILE_EDGE: f64 = 2.25;
/// Half-width of the "level zero" root cube.
pub const BASE_HALF: f64 = 2.25;
/// Octree depth limit (protection against degenerate inputs).
pub const MAX_DEPTH: u32 = 20;
/// Points per tile limit: the payload header record uses a `u16 index`, i.e. `0..=65535`.
pub const MAX_TILE_POINTS: u32 = 1 << 16;
/// Tile bbox is rounded to 1 mm.
pub const BBOX_ROUND: usize = 3;
/// `chunk_id` @`0xD0` in all our reference files (the old `level_7_orig` has 11).
pub const DEFAULT_CHUNK_ID: u32 = 131;
