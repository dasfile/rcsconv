//! Сборка `.rcs` из облака точек: октодерево, блоки, заголовок.
//!
//! Порт `tools/rcs_build.build_from_source` — все блоки генерируются, шаблон не
//! нужен (`.clinerules` п.14). Порядок узлов — DFS (`subtree_key`), внутри узла —
//! порядок точек файла; именно так получается побайтовый паритет с Python.

use std::collections::HashMap;

use rcs_format::header::{Header, GUID_OFFSET};
use rcs_format::point::{encode_body, Point};
use rcs_format::{metadata, num, properties, writer};

use crate::error::{Error, Result};
use crate::octree::{self, Cell};
use crate::point::CloudPoint;
use crate::progress::{NoProgress, Progress, Stage};
use crate::source::{Bounds, Source};
use crate::{BBOX_ROUND, DEFAULT_CHUNK_ID, MAX_TILE_POINTS};

/// Как часто отчитываться о прогрессе и проверять отмену, точек.
const REPORT_EVERY: usize = 1 << 16;

/// Как собирать `.rcs`.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Предел точек в узле (`1…65536`); по умолчанию — предел формата.
    pub max_per_tile: u32,
    /// Подменить GUID (по умолчанию — новый случайный).
    pub guid: Option<String>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions { max_per_tile: MAX_TILE_POINTS, guid: None }
    }
}

/// Что получилось (для отчёта CLI и журнала).
#[derive(Debug, Clone)]
pub struct Stats {
    /// `count` из заголовка источника.
    pub points: u64,
    /// Число узлов (записей `type 3`).
    pub tiles: usize,
    /// Глубина октодерева.
    pub depth: u32,
    /// Ребро плитки, м.
    pub tile_edge: f64,
    /// Полуширина корневого куба.
    pub root_half: f64,
    /// Центр bbox (он же `center` заголовка).
    pub center: [f64; 3],
    /// Полупротяжённость bbox (`extent`).
    pub extent: [f64; 3],
    /// Слова блока `type 1` — занятые узлы по уровням.
    pub metadata_words: Vec<u32>,
    /// Число 72-байтовых записей блока `type 1`.
    pub metadata_records: usize,
    /// Максимум точек в узле.
    pub max_points_per_tile: usize,
    /// Предел, с которым собирали.
    pub max_per_tile: u32,
    /// Размер файла, байт.
    pub size: usize,
}

/// Собрать `.rcs` из источника точек.
pub fn build_from_source(
    source: &mut dyn Source,
    options: &BuildOptions,
) -> Result<(Vec<u8>, Stats)> {
    build_from_source_with(source, options, &NoProgress)
}

/// Собрать `.rcs` из источника, отчитываясь о прогрессе и слушая отмену.
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

/// Собрать `.rcs` из готового набора точек.
pub fn build_from_points(
    points: &[CloudPoint],
    bounds: &Bounds,
    options: &BuildOptions,
) -> Result<(Vec<u8>, Stats)> {
    build_from_points_with(points, bounds, options, &NoProgress)
}

/// Собрать `.rcs` из готового набора точек, отчитываясь о прогрессе.
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

    // Группировка: ячейка → индексы точек в порядке файла.
    let mut cells: HashMap<Cell, Vec<u32>> = HashMap::new();
    for (index, point) in points.iter().enumerate() {
        let local = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        cells
            .entry(octree::cell_of(local, half, depth))
            .or_default()
            .push(index as u32);
        if index % REPORT_EVERY == REPORT_EVERY - 1 {
            progress.stage(Stage::Octree, index as u64 + 1, total);
            check_cancel(progress)?;
        }
    }
    progress.stage(Stage::Octree, total, total);
    check_cancel(progress)?;

    // DFS-порядок узлов.
    let mut order: Vec<(Vec<u8>, Cell)> = cells
        .keys()
        .map(|cell| (octree::subtree_key(*cell, depth), *cell))
        .collect();
    order.sort();

    let mut records: Vec<u8> = Vec::with_capacity(368 * order.len());
    let mut payloads: Vec<u8> = Vec::new();
    let mut max_points_per_tile = 0usize;

    let tile_total = order.len() as u64;
    progress.stage(Stage::Tiles, 0, tile_total);
    for (index, (_, cell)) in order.iter().enumerate() {
        let indices = &cells[cell];
        max_points_per_tile = max_points_per_tile.max(indices.len());

        let cube_min = [
            -half + cell[0] as f64 * edge,
            -half + cell[1] as f64 * edge,
            -half + cell[2] as f64 * edge,
        ];
        let cube_max = [cube_min[0] + edge, cube_min[1] + edge, cube_min[2] + edge];

        write_tile(&mut records, &mut payloads, points, indices, center, cube_min, cube_max)?;
        progress.stage(Stage::Tiles, index as u64 + 1, tile_total);
        check_cancel(progress)?;
    }

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

/// Прервать сборку, если пришла отмена.
fn check_cancel(progress: &dyn Progress) -> Result<()> {
    if progress.cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}


/// Запись узла в `type 3` и payload узла в `type 11`.
fn write_tile(
    records: &mut Vec<u8>,
    payloads: &mut Vec<u8>,
    points: &[CloudPoint],
    indices: &[u32],
    center: [f64; 3],
    cube_min: [f64; 3],
    cube_max: [f64; 3],
) -> Result<()> {
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
    Ok(())
}

/// Заголовок собираемого файла (Unified RCS: `chunk_version = 5`).
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
