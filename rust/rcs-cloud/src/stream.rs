//! Streaming writer: build the `.rcs` on disk without keeping the cloud in memory.
//!
//! See `docs/STREAMING.md`. The output is byte-identical to the in-memory writer.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rcs_format::header::{Header, GUID_OFFSET};
use rcs_format::{metadata, num, properties};

use crate::error::{Error, Result};
use crate::octree::{self, Cell};
use crate::point::CloudPoint;
use crate::progress::{Progress, Stage};
use crate::source::{Bounds, Source};
use crate::{BuildOptions, Stats};
use crate::{BBOX_ROUND, DEFAULT_CHUNK_ID, MAX_TILE_POINTS};

const BUCKET_TARGET: u64 = 64 * 1024 * 1024;
const MAX_BUCKETS: usize = 64;
const RECORD: usize = 20;
const REPORT_POINTS: u64 = 1 << 16;

/// Build the file at `target`; temporary files live next to it and are removed.
pub fn build_streaming(
    source: &mut dyn Source,
    options: &BuildOptions,
    progress: &dyn Progress,
    target: &Path,
) -> Result<Stats> {
    if options.max_per_tile == 0 || options.max_per_tile > MAX_TILE_POINTS {
        return Err(Error::MaxPerTile(options.max_per_tile));
    }

    let bounds = match source.bounds_hint() {
        Some(bounds) => bounds,
        None => scan_bounds(source, progress)?,
    };
    if bounds.count == 0 {
        return Err(Error::EmptyCloud);
    }

    let center = bounds.center();
    let half_extent = bounds.half_extent();
    let mut extent = [0.0f64; 3];
    for axis in 0..3 {
        extent[axis] = num::round_digits(half_extent[axis], BBOX_ROUND);
    }
    let half = octree::root_half(extent);

    progress.stage(Stage::Octree, 0, bounds.count);
    let mut depth = octree::recap_depth(half);
    let cells = loop {
        let cells = count_cells(source, center, half, depth, progress)?;
        if cells.values().all(|count| *count <= options.max_per_tile) {
            break cells;
        }
        depth += 1;
        if depth > crate::MAX_DEPTH {
            return Err(Error::DepthExceeded);
        }
    };

    progress.stage(Stage::Octree, bounds.count, bounds.count);
    let mut order: Vec<(Vec<u8>, Cell)> = cells
        .keys()
        .map(|cell| (octree::subtree_key(*cell, depth), *cell))
        .collect();
    order.sort();
    let tiles = order.len();
    let cells_list: Vec<Cell> = order.iter().map(|(_, cell)| *cell).collect();
    let mut tile_of: HashMap<Cell, u32> = HashMap::with_capacity(tiles);
    for (index, (_, cell)) in order.iter().enumerate() {
        tile_of.insert(*cell, index as u32);
    }
    let counts: Vec<u32> = order.iter().map(|(_, cell)| cells[cell]).collect();
    let edge = (2.0 * half) / (1_u64 << depth) as f64;

    let words = octree::level_counts(&cells_list, depth);
    let metadata_block = metadata::build_metadata_block(&words, None)?;
    let properties_block = properties::build_default_properties_block();

    let buckets = bucket_count(bounds.count);
    let temp = temp_dir(target);
    let guard = TempGuard(temp.clone());
    fs::create_dir_all(&temp)?;

    let mut mins = vec![[f64::INFINITY; 3]; tiles];
    let mut maxs = vec![[f64::NEG_INFINITY; 3]; tiles];
    let mut writers: Vec<BufWriter<File>> = Vec::with_capacity(buckets);
    for index in 0..buckets {
        let path = temp.join(format!("bucket-{index:04}.bin"));
        writers.push(BufWriter::with_capacity(256 * 1024, File::create(path)?));
    }

    let mut scanned = 0u64;
    progress.stage(Stage::Tiles, 0, bounds.count);
    source.scan(progress, &mut |point| {
        let local = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let cell = octree::cell_of(local, half, depth);
        let tile = *tile_of
            .get(&cell)
            .ok_or_else(|| Error::Source(format!("cell {:?} is missing", cell)))?;
        let cube_min = [
            -half + cell[0] as f64 * edge,
            -half + cell[1] as f64 * edge,
            -half + cell[2] as f64 * edge,
        ];
        let body = encode_point(&point, &center, &cube_min)?;
        let bucket = ((tile as u64 * buckets as u64) / tiles as u64) as usize;
        let writer = &mut writers[bucket];
        writer.write_all(&tile.to_le_bytes())?;
        writer.write_all(&body)?;
        for axis in 0..3 {
            if local[axis] < mins[tile as usize][axis] {
                mins[tile as usize][axis] = local[axis];
            }
            if local[axis] > maxs[tile as usize][axis] {
                maxs[tile as usize][axis] = local[axis];
            }
        }
        scanned += 1;
        if scanned % REPORT_POINTS == 0 {
            progress.stage(Stage::Tiles, scanned, bounds.count);
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    })?;
    progress.stage(Stage::Tiles, bounds.count, bounds.count);
    if scanned != bounds.count {
        return Err(Error::Source(format!(
            "scanned {scanned} points, the header declares {}",
            bounds.count
        )));
    }
    drop(writers);

    let guid = match &options.guid {
        Some(text) => rcs_format::header::normalize_guid(text)?,
        None => crate::guid::new_guid(),
    };
    let header = Header {
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
        guid: guid.clone(),
        guid_offset: GUID_OFFSET,
    };
    let header_bytes = header.to_bytes(Some(&guid), 4)?;

    let type3_size = 8u64 + 368 * tiles as u64;
    let type1_size = metadata_block.len() as u64;
    let type11_size: u64 = counts
        .iter()
        .map(|count| RECORD as u64 * *count as u64)
        .sum();
    let type0_size = properties_block.len() as u64;
    let blocks: [(u64, u64); 4] = [
        (3, type3_size),
        (1, type1_size),
        (11, type11_size),
        (0, type0_size),
    ];

    let tmp = tmp_path(target);
    let built: Result<()> = (|| {
        let mut out = BufWriter::with_capacity(8 << 20, File::create(&tmp)?);
        out.write_all(&header_bytes)?;
        let mut offset = 0x1812Du64;
        for (kind, size) in blocks {
            out.write_all(&offset.to_le_bytes())?;
            out.write_all(&size.to_le_bytes())?;
            out.write_all(&kind.to_le_bytes())?;
            offset += size;
        }
        let mut written = 0x135u64 + 24 * 4;
        let zeros = [0u8; 4096];
        while written < 0x1812D {
            let chunk = (0x1812D - written).min(zeros.len() as u64) as usize;
            out.write_all(&zeros[..chunk])?;
            written += chunk as u64;
        }

        progress.stage(Stage::Write, 0, 1);
        out.write_all(&(tiles as u64).to_le_bytes())?;
        for (index, (_, cell)) in order.iter().enumerate() {
            let count = counts[index];
            let cube_min = [
                -half + cell[0] as f64 * edge,
                -half + cell[1] as f64 * edge,
                -half + cell[2] as f64 * edge,
            ];
            let cube_max = [cube_min[0] + edge, cube_min[1] + edge, cube_min[2] + edge];
            let bbox_min = [
                num::round_digits(mins[index][0], BBOX_ROUND),
                num::round_digits(mins[index][1], BBOX_ROUND),
                num::round_digits(mins[index][2], BBOX_ROUND),
            ];
            let bbox_max = [
                num::round_digits(maxs[index][0], BBOX_ROUND),
                num::round_digits(maxs[index][1], BBOX_ROUND),
                num::round_digits(maxs[index][2], BBOX_ROUND),
            ];
            for triple in [cube_min, cube_max, bbox_min, bbox_max] {
                for value in triple {
                    out.write_all(&value.to_le_bytes())?;
                }
            }
            let payload_size = RECORD as u32 * count;
            for word in [1u32, count, payload_size, 0, 0, 0, 0, 0] {
                out.write_all(&word.to_le_bytes())?;
            }
            for word_index in 0..60 {
                let word = if word_index == 27 { count } else { 0 };
                out.write_all(&word.to_le_bytes())?;
            }
        }
        out.write_all(&metadata_block)?;

        let mut seen = vec![0u32; tiles];
        let mut current: Option<u32> = None;
        for bucket in 0..buckets {
            let path = temp.join(format!("bucket-{bucket:04}.bin"));
            let mut data = Vec::new();
            File::open(path)?.read_to_end(&mut data)?;
            let records = data.len() / RECORD;
            let mut indices: Vec<u32> = (0..records as u32).collect();
            indices.sort_by_key(|index| {
                let start = *index as usize * RECORD;
                u32::from_le_bytes([
                    data[start],
                    data[start + 1],
                    data[start + 2],
                    data[start + 3],
                ])
            });
            for index in indices {
                let start = index as usize * RECORD;
                let tile = u32::from_le_bytes([
                    data[start],
                    data[start + 1],
                    data[start + 2],
                    data[start + 3],
                ]);
                if current != Some(tile) {
                    current = Some(tile);
                    let count = counts[tile as usize];
                    for number in 0..count {
                        out.write_all(&(number as u16).to_le_bytes())?;
                        out.write_all(&[0u8, 8])?;
                    }
                }
                out.write_all(&data[start + 4..start + RECORD])?;
                seen[tile as usize] += 1;
            }
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
            progress.stage(Stage::Write, bucket as u64 + 1, buckets as u64);
        }
        for (index, expected) in counts.iter().enumerate() {
            if seen[index] != *expected {
                return Err(Error::Source(format!(
                    "tile {index}: {} points written, {expected} expected",
                    seen[index]
                )));
            }
        }
        out.write_all(&properties_block)?;
        out.flush()?;
        Ok(())
    })();

    if let Err(error) = built {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(&tmp, target)?;
    let size = fs::metadata(target).map(|meta| meta.len() as usize).unwrap_or(0);

    let max_points_per_tile = counts.iter().copied().max().unwrap_or(0) as usize;
    let metadata_records = words.iter().map(|word| *word as usize).sum();
    let stats = Stats {
        points: bounds.count,
        tiles,
        depth,
        tile_edge: edge,
        root_half: half,
        center,
        extent,
        metadata_words: words,
        metadata_records,
        max_points_per_tile,
        max_per_tile: options.max_per_tile,
        size,
    };
    drop(guard);
    Ok(stats)
}

fn scan_bounds(source: &mut dyn Source, progress: &dyn Progress) -> Result<Bounds> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut count = 0u64;
    source.scan(progress, &mut |point| {
        for axis in 0..3 {
            let value = point.xyz()[axis];
            if value < low[axis] {
                low[axis] = value;
            }
            if value > high[axis] {
                high[axis] = value;
            }
        }
        count += 1;
        Ok(())
    })?;
    if count == 0 {
        return Err(Error::EmptyCloud);
    }
    Ok(Bounds { low, high, count })
}

fn count_cells(
    source: &mut dyn Source,
    center: [f64; 3],
    half: f64,
    depth: u32,
    progress: &dyn Progress,
) -> Result<HashMap<Cell, u32>> {
    let mut cells: HashMap<Cell, u32> = HashMap::new();
    source.scan(progress, &mut |point| {
        let local = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let cell = octree::cell_of(local, half, depth);
        *cells.entry(cell).or_insert(0) += 1;
        Ok(())
    })?;
    Ok(cells)
}

fn encode_point(
    point: &CloudPoint,
    center: &[f64; 3],
    cube_min: &[f64; 3],
) -> Result<[u8; 16]> {
    let local = rcs_format::point::Point {
        x: point.x - center[0],
        y: point.y - center[1],
        z: point.z - center[2],
        r: point.r,
        g: point.g,
        b: point.b,
        intensity: point.intensity,
        extra: [0; 6],
    };
    Ok(rcs_format::point::encode_body(&local, *cube_min)?)
}

fn bucket_count(points: u64) -> usize {
    let per_bucket = (BUCKET_TARGET / RECORD as u64).max(1);
    let buckets = points / per_bucket + 1;
    (buckets as usize).clamp(1, MAX_BUCKETS)
}

fn temp_dir(target: &Path) -> PathBuf {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.rcs".to_string());
    parent.join(format!(".{name}.buckets{}", std::process::id()))
}

fn tmp_path(target: &Path) -> PathBuf {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.rcs".to_string());
    parent.join(format!(".{name}.tmp{}", std::process::id()))
}

struct TempGuard(PathBuf);

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
