//! Conversion orchestration: the shared part of the CLI (`rcsconv`) and GUI (`rcsconv-gui`).
//!
//! There is no console output and no windows here: the layer returns a result
//! ([`Conversion`]) and the interface decides where to show it. The interface
//! rules mirror the Python reference (`tools/rcs_cli.py`, README): input is a
//! file, directory or mask; the output name is `<name>.rcs`; an existing file is
//! not overwritten without `--force`.

#![forbid(unsafe_code)]

pub mod convert;
pub mod files;
pub mod options;

pub use convert::{convert_one, convert_to, describe, write_atomic, Conversion, Failure, Status};
pub use files::{collect_dir, default_target, expand_sources, glob, has_suffix, wildcard_match};
pub use options::{ArgsError, Options};

/// Progress and cancellation — re-exported from `rcs-cloud` so the GUI depends on one crate.
pub use rcs_cloud::progress::{NoProgress, Progress, Stage};
pub use rcs_cloud::{BuildOptions, Stats};

/// LAS/LAZ input settings — the GUI builds them from window fields (as the CLI from flags).
pub use cloud_las::{LasOptions, Recenter, ScaleMode};
