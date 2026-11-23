//! Fixed-depth octree: cells, depth, per-level counters.
//!
//! Port of `tools/rcs_build.py` (`root_half`, `recap_depth`, `cell_of`,
//! `subtree_key`, `choose_depth`, `level_counts`). CONFIRMED
//! (`docs/RCS-FORMAT.md` 4.3, 6): ReCap picks the depth so that the tile edge is
//! about 2.25 m, and walks tiles in DFS order `x + 2y + 4z`.

use std::collections::{HashMap, HashSet};

use rcs_format::num;

use crate::error::{Error, Result};
use crate::point::CloudPoint;
use crate::{BASE_HALF, MAX_DEPTH, TILE_EDGE};

/// Octree cell: per-axis indices at the given depth.
pub type Cell = [i64; 3];

/// Half-width of the root cube: `2.25·2^k`, the smallest one covering the bbox.
pub fn root_half(half_extent: [f64; 3]) -> f64 {
    let need = half_extent.iter().fold(0.0f64, |acc, value| acc.max(value.abs()));
    let mut half = BASE_HALF;
    while half < need - 1e-9 {
        half *= 2.0;
    }
    half
}

/// Depth at which the tile edge equals 2.25 m (the way ReCap chooses it).
pub fn recap_depth(half: f64) -> u32 {
    let value = ((2.0 * half) / TILE_EDGE).log2();
    let rounded = num::round_half_even(value);
    if rounded <= 0.0 {
        0
    } else {
        rounded as u32
    }
}

/// Cell index at the given depth (0..2^depth-1 on each axis).
pub fn cell_of(local: [f64; 3], half: f64, depth: u32) -> Cell {
    let side = 1_i64 << depth;
    let edge = (2.0 * half) / side as f64;
    let mut cell = [0_i64; 3];
    for axis in 0..3 {
        let raw = ((local[axis] + half) / edge).floor();
        let clamped = raw.max(0.0).min((side - 1) as f64);
        cell[axis] = clamped as i64;
    }
    cell
}

/// DFS order key: octant indices from root to leaf (`x + 2y + 4z`).
pub fn subtree_key(cell: Cell, depth: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(depth as usize);
    for level in (1..=depth).rev() {
        let mask = 1_i64 << (level - 1);
        let x = i8::from(cell[0] & mask != 0);
        let y = i8::from(cell[1] & mask != 0);
        let z = i8::from(cell[2] & mask != 0);
        key.push((x + 2 * y + 4 * z) as u8);
    }
    key
}

/// Increase the depth until no cell holds more than `max_per_tile` points.
pub fn choose_depth(
    points: &[CloudPoint],
    center: [f64; 3],
    half: f64,
    max_per_tile: u32,
) -> Result<u32> {
    let mut depth = recap_depth(half);
    while depth <= MAX_DEPTH {
        let mut seen: HashMap<Cell, u32> = HashMap::new();
        let mut collision = false;
        for point in points {
            let local = [
                point.x - center[0],
                point.y - center[1],
                point.z - center[2],
            ];
            let counter = seen.entry(cell_of(local, half, depth)).or_insert(0);
            *counter += 1;
            if *counter > max_per_tile {
                collision = true;
                break;
            }
        }
        if !collision {
            return Ok(depth);
        }
        depth += 1;
    }
    Err(Error::DepthExceeded)
}

/// Number of occupied octree tiles on every level `0..=depth`.
///
/// The parent of a leaf `cell` at level `L` is `cell >> (depth - L)`; these are
/// the numbers ReCap writes into the `type 1` block header.
pub fn level_counts(cells: &[Cell], depth: u32) -> Vec<u32> {
    if cells.is_empty() {
        return Vec::new();
    }
    let mut counts = Vec::with_capacity(depth as usize + 1);
    for level in 0..=depth {
        let shift = depth - level;
        let mut parents: HashSet<Cell> = HashSet::new();
        for cell in cells {
            parents.insert([cell[0] >> shift, cell[1] >> shift, cell[2] >> shift]);
        }
        counts.push(parents.len() as u32);
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_half_covers_extent() {
        assert_eq!(root_half([0.667, 1.5, 1.056]), 2.25);
        assert_eq!(root_half([2.25, 0.0, 0.0]), 2.25);
        assert_eq!(root_half([2.26, 0.0, 0.0]), 4.5);
        assert_eq!(root_half([10.0, 0.0, 0.0]), 18.0);
    }

    #[test]
    fn recap_depth_is_tile_edge() {
        assert_eq!(recap_depth(2.25), 1);
        assert_eq!(recap_depth(4.5), 2);
        assert_eq!(recap_depth(18.0), 4);
    }

    #[test]
    fn cells_and_keys() {
        assert_eq!(cell_of([-2.25, -2.25, -2.25], 2.25, 1), [0, 0, 0]);
        assert_eq!(cell_of([2.24, 2.24, 2.24], 2.25, 1), [1, 1, 1]);
        assert_eq!(subtree_key([1, 0, 1], 1), vec![5]);
        // depth 2, cell [0,1,1]: octant 0 at level 2 (high bits are zero),
        // x + 2y + 4z = 0 + 2 + 4 = 6 at level 1.
        assert_eq!(subtree_key([0, 1, 1], 2), vec![0, 6]);
    }

    #[test]
    fn level_counts_of_probe6() {
        // probe6: three leaves at depth 1 -> levels [1, 3].
        assert_eq!(level_counts(&[[0, 0, 0], [1, 0, 0], [0, 1, 1]], 1), vec![1, 3]);
    }
}
