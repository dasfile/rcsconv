//! Point bodies (`type 11`): 16 bytes per point.
//!
//! CONFIRMED on `probe6`, `test1`, `test2` and tile 37 of `level_7`
//! (`docs/RCS-FORMAT.md` 4.2, 5):
//!
//! ```text
//! [0:2]  u16  x: (x - cube_min.x) in 1 mm
//! [2:4]  u16  y: (y - cube_min.y) in 1/4 mm   (scale 4)
//! [4:6]  u16  z: (z - cube_min.z) in 1/16 mm  (scale 16)
//! [6:8]  u16  not decoded (0 in the reference files)
//! [8]    u8   intensity (0..255)
//! [9:12] u8×3 colour in B, G, R order
//! [12:16]     not decoded (0 in the reference files)
//! ```
//!
//! Tiles with several points: payload = header + `count` records of 16 or 20
//! bytes; the layout is chosen by matching the bbox against the tile record.
//!
//! [`Point`] coordinates are **local** (relative to the tile `cube_min`). World
//! coordinates are obtained by adding `header.center` — [`Point::world`]; that is
//! what `tools/rcs2pts.py` does (the `--local` flag prints local coordinates).

use crate::container::Container;
use crate::error::{Error, Result};
use crate::tile::{tile_payload, Tile};

/// How many field units make 1 mm on the x, y, z axes.
pub const AXIS_SCALE: [f64; 3] = [1.0, 4.0, 16.0];
/// Point body size.
pub const BODY_SIZE: usize = 16;
/// Alternative record size (4 undecoded bytes at the start).
pub const WIDE_STRIDE: usize = 20;
/// bbox matching tolerance when choosing the layout, metres.
pub const BBOX_TOLERANCE: f64 = 2e-3;

/// Cloud point: coordinates in metres, RGB colour, intensity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X, metres.
    pub x: f64,
    /// Y, metres.
    pub y: f64,
    /// Z, metres.
    pub z: f64,
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Intensity (`[8]`).
    pub intensity: u8,
    /// Undecoded bytes `[6], [7], [12..16]` — carried over as is.
    pub extra: [u8; 6],
}

impl Point {
    /// Coordinates as an array (local, without the centre).
    pub fn xyz(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// World coordinates: local + `header.center` (as in `rcs2pts.py`; the
    /// `--local` flag in the Python version prints these coordinates).
    pub fn world(&self, center: [f64; 3]) -> [f64; 3] {
        [
            self.x + center[0],
            self.y + center[1],
            self.z + center[2],
        ]
    }
}

/// Decode a 16-byte point body.
pub fn decode_body(body: &[u8], cube_min: [f64; 3]) -> Result<Point> {
    if body.len() != BODY_SIZE {
        return Err(Error::BodySize(body.len()));
    }
    let raw = [
        u16::from_le_bytes([body[0], body[1]]),
        u16::from_le_bytes([body[2], body[3]]),
        u16::from_le_bytes([body[4], body[5]]),
    ];
    let coord = |axis: usize| cube_min[axis] + raw[axis] as f64 / AXIS_SCALE[axis] / 1000.0;
    Ok(Point {
        x: coord(0),
        y: coord(1),
        z: coord(2),
        r: body[11],
        g: body[10],
        b: body[9],
        intensity: body[8],
        extra: [body[6], body[7], body[12], body[13], body[14], body[15]],
    })
}

/// Compose a 16-byte point body (the inverse of [`decode_body`]).
///
/// Quantisation repeats `tools/rcs_build.py`: the delta to `cube_min` is first
/// rounded to 1 mm, then multiplied by the axis scale and rounded to an integer
/// (both roundings are Python-style half-to-even, see [`crate::num`]).
/// A value that does not fit into `u16` gives [`Error::PointOutOfCube`].
pub fn encode_body(point: &Point, cube_min: [f64; 3]) -> Result<[u8; BODY_SIZE]> {
    let mut out = [0u8; BODY_SIZE];
    for axis in 0..3 {
        let delta = crate::num::round_digits(point.xyz()[axis] - cube_min[axis], 3);
        let quantized = crate::num::round_half_even(delta * 1000.0 * AXIS_SCALE[axis]);
        if !(0.0..=u16::MAX as f64).contains(&quantized) {
            return Err(Error::PointOutOfCube {
                axis,
                value: point.xyz()[axis],
            });
        }
        let bytes = (quantized as u16).to_le_bytes();
        out[axis * 2] = bytes[0];
        out[axis * 2 + 1] = bytes[1];
    }
    out[6] = point.extra[0];
    out[7] = point.extra[1];
    out[8] = point.intensity;
    out[9] = point.b;
    out[10] = point.g;
    out[11] = point.r;
    out[12..].copy_from_slice(&point.extra[2..]);
    Ok(out)
}

/// Decode all points of a tile, choosing the record stride by the tile bbox.
pub fn decode_tile(container: &Container<'_>, tile: &Tile) -> Result<Vec<Point>> {
    let payload = tile_payload(container, tile)?;
    if tile.count == 0 {
        return Ok(Vec::new());
    }
    let count = tile.count as usize;
    for stride in [BODY_SIZE, WIDE_STRIDE] {
        let Some(body_bytes) = count.checked_mul(stride) else {
            continue;
        };
        let Some(header) = payload.len().checked_sub(body_bytes) else {
            continue;
        };
        let mut points = Vec::with_capacity(count);
        for index in 0..count {
            let start = header + index * stride + (stride - BODY_SIZE);
            points.push(decode_body(&payload[start..start + BODY_SIZE], tile.cube_min)?);
        }
        if bbox_matches(tile, &points) {
            return Ok(points);
        }
    }
    Err(Error::PayloadLayout {
        tile: tile.index,
        size: tile.payload_size as u64,
        count: tile.count,
    })
}

/// Decode every tile of the container.
pub fn decode_all(container: &Container<'_>) -> Result<Vec<Point>> {
    let mut points = Vec::new();
    for tile in crate::tile::iter_tiles(container)? {
        points.extend(decode_tile(container, &tile)?);
    }
    Ok(points)
}

/// Does the point bbox match the tile record bbox (tolerance `BBOX_TOLERANCE`)?
fn bbox_matches(tile: &Tile, points: &[Point]) -> bool {
    if points.is_empty() {
        return false;
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point.xyz()[axis]);
            high[axis] = high[axis].max(point.xyz()[axis]);
        }
    }
    (0..3).all(|axis| {
        (low[axis] - tile.points_min[axis]).abs() <= BBOX_TOLERANCE
            && (high[axis] - tile.points_max[axis]).abs() <= BBOX_TOLERANCE
    })
}
