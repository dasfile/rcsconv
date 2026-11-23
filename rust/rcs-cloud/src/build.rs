//! Building a `.rcs` from a point cloud: octree, blocks, header.
//!
//! Port of `tools/rcs_build.build_from_source`: every block is generated, no
//! template is needed (`.clinerules` 14). Tile order is DFS (`subtree_key`),
//! points inside a tile keep file order; this is what makes the byte parity
//! with the Python reference work.

use std::collections::HashMap;

use rayon::prelude::*;

use rcs_format::header::{Header, GUID_OFFSET};
use rcs_format::point::{encode_body, Point};
use rcs_format::{metadata, num, properties, writer};

use crate::error::{Error, Result};
use crate::octree::{self, Cell};
use crate::point::CloudPoint;
use crate::progress::{NoProgress, Progress, Stage};
use crate::source::{Bounds, Source};
use crate::{BBOX_ROUND, DEFAULT_CHUNK_ID, MAX_TILE_POINTS};


/// How to build the `.rcs`.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Points per tile limit (`1..=65536`); the format limit by default.
    pub max_per_tile: u32,
    /// Replace the GUID (a new random one by default).
    pub guid: Option<String>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions { max_per_tile: MAX_TILE_POINTS, guid: None }
    }
}

/// What was produced (for the CLI report and the log).
#[derive(Debug, Clone)]
pub struct Stats {
    /// `count` from the source header.
    pub points: u64,
    /// Number of tiles (`type 3` records).
    pub tiles: usize,
    /// Octree depth.
    pub depth: u32,
    /// Tile edge, metres.
    pub tile_edge: f64,
    /// Half-width of the root cube.
    pub root_half: f64,
    /// bbox centre (also the header `center`).
    pub center: [f64; 3],
    /// bbox half-extent (`extent`).
    pub extent: [f64; 3],
    /// Words of the `type 1` block — occupied tiles per level.
    pub metadata_words: Vec<u32>,
    /// Number of 72-byte records in the `type 1` block.
    pub metadata_records: usize,
    /// Maximum points in a tile.
    pub max_points_per_tile: usize,
    /// Limit used for the build.
    pub max_per_tile: u32,
    /// File size, bytes.
    pub size: usize,
}

/// Build a `.rcs` from a point source.
pub fn build_from_source(
    source: &mut dyn Source,
    options: &BuildOptions,
) -> Result<(Vec<u8>, Stats)> {
    build_from_source_with(source, options, &NoProgress)
}

/// Build a `.rcs` from a source while reporting progress and listening for cancellation.
pub fn build_from_source_with(
    source: &mut dyn Source,
    options: &BuildOptions,
    progress: &dyn Progress,
) -> Result<(Vec<u8>, Stats)> {
    let bounds = source.bounds_with(progress)?;
    check_cancel(progress)?;
    progress.stage(Stage::Read, 0, bounds.count);
    let points = source.points_with(progress)?;
    progress.stage(Stage::Read, points.len() as u64, bounds.count);
    check_cancel(progress)?;
    build_from_points_with(&points, &bounds, options, progress)
}

/// Build a `.rcs` from an already loaded point set.
pub fn build_from_points(
    points: &[CloudPoint],
    bounds: &Bounds,
    options: &BuildOptions,
) -> Result<(Vec<u8>, Stats)> {
    build_from_points_with(points, bounds, options, &NoProgress)
}

/// Build a `.rcs` from an already loaded point set while reporting progress.
pub fn build_from_points_with(
    points: &[CloudPoint],
    bounds: &Bounds,
    options: &BuildOptions,
    progress: &dyn Progress,
) -> Result<(Vec<u8>, Stats)> {
    if points.is_empty() {
        return Err(Error::EmptyCloud);
    }
    if options.max_per_tile == 0 || options.max_per_tile > MAX_TILE_POINTS {
        return Err(Error::MaxPerTile(options.max_per_tile));
    }
    let total = points.len() as u64;
    progress.stage(Stage::Octree, 0, total);

    let center = bounds.center();
    let half_extent = bounds.half_extent();
    let mut extent = [0.0f64; 3];
    for axis in 0..3 {
        extent[axis] = num::round_digits(half_extent[axis], BBOX_ROUND);
    }
    let half = octree::root_half(extent);
    let depth = octree::choose_depth(points, center, half, options.max_per_tile)?;
    let edge = (2.0 * half) / (1_u64 << depth) as f64;

    check_cancel(progress)?;

    // Grouping: cell -> point indices in file order. Chunks are processed in
    // parallel and merged in chunk order, so the index lists stay ordered.
    let threads = rayon::current_num_threads().max(1);
    let chunk = (points.len() / (threads * 4)).max(1);
    let parts_count = (points.len() + chunk - 1) / chunk;
    let parts: Vec<HashMap<Cell, Vec<u32>>> = (0..parts_count)
        .into_par_iter()
        .map(|part| {
            let start = part * chunk;
            let end = ((part + 1) * chunk).min(points.len());
            let mut local: HashMap<Cell, Vec<u32>> = HashMap::new();
            for index in start..end {
                let point = &points[index];
                let shifted = [
                    point.x - center[0],
                    point.y - center[1],
                    point.z - center[2],
                ];
                local
                    .entry(octree::cell_of(shifted, half, depth))
                    .or_default()
                    .push(index as u32);
            }
            local
        })
        .collect();
    let mut cells: HashMap<Cell, Vec<u32>> = HashMap::new();
    for part in parts {
        for (cell, mut indices) in part {
            cells.entry(cell).or_default().append(&mut indices);
        }
    }
    progress.stage(Stage::Octree, total, total);
    check_cancel(progress)?;

    // DFS tile order.
    let mut order: Vec<(Vec<u8>, Cell)> = cells
        .keys()
        .map(|cell| (octree::subtree_key(*cell, depth), *cell))
        .collect();
    order.sort();

    let max_points_per_tile = order
        .iter()
        .map(|(_, cell)| cells[cell].len())
        .max()
        .unwrap_or(0);

    let tile_total = order.len() as u64;
    progress.stage(Stage::Tiles, 0, tile_total);
    let encoded: Vec<Result<(Vec<u8>, Vec<u8>)>> = order
        .par_iter()
        .map(|(_, cell)| {
            let indices = &cells[cell];
            let cube_min = [
                -half + cell[0] as f64 * edge,
                -half + cell[1] as f64 * edge,
                -half + cell[2] as f64 * edge,
            ];
            let cube_max = [cube_min[0] + edge, cube_min[1] + edge, cube_min[2] + edge];
            encode_tile(points, indices, center, cube_min, cube_max)
        })
        .collect();

    let mut records: Vec<u8> = Vec::with_capacity(368 * order.len());
    let mut payloads: Vec<u8> = Vec::with_capacity(20 * points.len());
    for item in encoded {
        let (tile_records, tile_payloads) = item?;
        records.extend_from_slice(&tile_records);
        payloads.extend_from_slice(&tile_payloads);
    }
    progress.stage(Stage::Tiles, tile_total, tile_total);
    check_cancel(progress)?;

    progress.stage(Stage::Write, 0, 1);
    check_cancel(progress)?;

    let mut block3 = Vec::with_capacity(8 + records.len());
    block3.extend_from_slice(&(order.len() as u64).to_le_bytes());
    block3.extend_from_slice(&records);

    let cell_list: Vec<Cell> = order.iter().map(|(_, cell)| *cell).collect();
    let words = octree::level_counts(&cell_list, depth);
    let metadata_block = metadata::build_metadata_block(&words, None)?;

    let guid = match &options.guid {
        Some(text) => rcs_format::header::normalize_guid(text)?,
        None => crate::guid::new_guid(),
    };
    let header = build_header(bounds, center, extent, half, &guid);
    let blocks = [
        (3u64, block3),
        (1, metadata_block),
        (11, payloads),
        (0, properties::build_default_properties_block()),
    ];
    let blob = writer::build_file(&header, &blocks, Some(&guid))?;
    progress.stage(Stage::Write, 1, 1);

    let stats = Stats {
        points: bounds.count,
        tiles: order.len(),
        depth,
        tile_edge: edge,
        root_half: half,
        center,
        extent,
        metadata_records: words.iter().map(|word| *word as usize).sum(),
        metadata_words: words,
        max_points_per_tile,
        max_per_tile: options.max_per_tile,
        size: blob.len(),
    };
    Ok((blob, stats))
}

/// Stop the build when a cancellation arrives.
fn check_cancel(progress: &dyn Progress) -> Result<()> {
    if progress.cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}


/// Tile record in `type 3` and the tile payload in `type 11`.
fn encode_tile(
    points: &[CloudPoint],
    indices: &[u32],
    center: [f64; 3],
    cube_min: [f64; 3],
    cube_max: [f64; 3],
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut records: Vec<u8> = Vec::with_capacity(368);
    let mut payloads: Vec<u8> = Vec::with_capacity(20 * indices.len());
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for &index in indices {
        let point = &points[index as usize];
        for axis in 0..3 {
            let local = point.xyz()[axis] - center[axis];
            low[axis] = low[axis].min(local);
            high[axis] = high[axis].max(local);
        }
    }
    let mut bbox_min = [0.0f64; 3];
    let mut bbox_max = [0.0f64; 3];
    for axis in 0..3 {
        bbox_min[axis] = num::round_digits(low[axis], BBOX_ROUND);
        bbox_max[axis] = num::round_digits(high[axis], BBOX_ROUND);
    }

    let count = indices.len();
    let payload_len = 4 * count + 16 * count;
    payloads.reserve(payload_len);
    for number in 0..count {
        payloads.extend_from_slice(&(number as u16).to_le_bytes());
        payloads.push(0);
        payloads.push(8);
    }
    for &index in indices {
        let point = &points[index as usize];
        let local = Point {
            x: point.x - center[0],
            y: point.y - center[1],
            z: point.z - center[2],
            r: point.r,
            g: point.g,
            b: point.b,
            intensity: point.intensity,
            extra: [0; 6],
        };
        payloads.extend_from_slice(&encode_body(&local, cube_min)?);
    }

    for triple in [cube_min, cube_max, bbox_min, bbox_max] {
        for value in triple {
            records.extend_from_slice(&value.to_le_bytes());
        }
    }
    for word in [1u32, count as u32, payload_len as u32, 0, 0, 0, 0, 0] {
        records.extend_from_slice(&word.to_le_bytes());
    }
    let mut tail = [0u32; 60];
    tail[27] = count as u32;
    for word in tail {
        records.extend_from_slice(&word.to_le_bytes());
    }
    Ok((records, payloads))
}

/// Header of the file being built (Unified RCS: `chunk_version = 5`).
fn build_header(
    bounds: &Bounds,
    center: [f64; 3],
    extent: [f64; 3],
    half: f64,
    guid: &str,
) -> Header {
    Header {
        word4: 84,
        word8: 3,
        word12: 2,
        center,
        zeros28: [0.0; 3],
        unit: [1.0; 3],
        center2: center,
        root_min: [-half; 3],
        root_max: [half; 3],
        extent_min: [-extent[0], -extent[1], -extent[2]],
        extent_max: extent,
        chunk_id: DEFAULT_CHUNK_ID,
        chunk_version: 5,
        count: bounds.count,
        flags: [1, 1, 0],
        guid: guid.to_string(),
        guid_offset: GUID_OFFSET,
    }
}
