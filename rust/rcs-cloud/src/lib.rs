//! Слой облака точек: точка, источник, октодерево, сборка `.rcs`.
//!
//! Порт `tools/cloud.py` + `tools/rcs_build.py` (эталон — Python, паритет
//! проверяется побайтово, `docs/ROADMAP.md` R2/R4).
//!
//! Модуль о формате входа ничего не знает: источник точек реализует
//! [`Source`], поэтому `.pts` (`cloud-pts`) и `.las/.laz` (`cloud-las`)
//! подключаются снаружи. Результат — Unified RCS (диалект v5, `chunk_version = 5`).

#![forbid(unsafe_code)]

pub mod build;
pub mod error;
pub mod guid;
pub mod octree;
pub mod point;
pub mod progress;
pub mod source;

pub use build::{build_from_points, build_from_points_with, build_from_source, build_from_source_with, BuildOptions, Stats};
pub use error::{Error, Result};
pub use point::CloudPoint;
pub use progress::{NoProgress, Progress, Stage};
pub use rcs_format::num::round_half_even;
pub use rcs_format::header::normalize_guid;
pub use source::{Bounds, Source};

/// Ребро плитки, м: одинаково во всех эталонах ReCap.
pub const TILE_EDGE: f64 = 2.25;
/// Полуширина корневого куба «нулевого» уровня.
pub const BASE_HALF: f64 = 2.25;
/// Предел глубины октодерева (защита от вырожденных входов).
pub const MAX_DEPTH: u32 = 20;
/// Предел точек в узле: запись заголовка payload'а — `u16 index`, т.е. `0…65535`.
pub const MAX_TILE_POINTS: u32 = 1 << 16;
/// bbox узла округляется до 1 мм.
pub const BBOX_ROUND: usize = 3;
/// `chunk_id` @`0xD0` у всех наших эталонов (у старого `level_7_orig` — 11).
pub const DEFAULT_CHUNK_ID: u32 = 131;
