//! Тела точек (`type 11`): 16 байт на точку.
//!
//! ПОДТВЕРЖДЕНО на `probe6`, `test1`, `test2` и узле 37 `level_7`
//! (`docs/RCS-FORMAT.md` §4.2, §5):
//!
//! ```text
//! [0:2]  u16  x: (x − cube_min.x) в 1 мм
//! [2:4]  u16  y: (y − cube_min.y) в 1/4 мм   (масштаб 4)
//! [4:6]  u16  z: (z − cube_min.z) в 1/16 мм  (масштаб 16)
//! [6:8]  u16  не расшифровано (в эталонах 0)
//! [8]    u8   интенсивность (0..255)
//! [9:12] u8×3 цвет в порядке B, G, R
//! [12:16]     не расшифровано (в эталонах 0)
//! ```
//!
//! Узлы с несколькими точками: payload = заголовок + `count` записей по 16 или
//! 20 байт; раскладка подбирается по совпадению bbox с записью узла.
//!
//! Координаты [`Point`] — **локальные** (от `cube_min` узла). Мировые координаты
//! получаются прибавлением `header.center` — [`Point::world`]; именно так работает
//! `tools/rcs2pts.py` (флаг `--local` печатает локальные).

use crate::container::Container;
use crate::error::{Error, Result};
use crate::tile::{tile_payload, Tile};

/// Сколько единиц поля приходится на 1 мм по осям x, y, z.
pub const AXIS_SCALE: [f64; 3] = [1.0, 4.0, 16.0];
/// Размер тела точки.
pub const BODY_SIZE: usize = 16;
/// Альтернативный размер записи (4 нерасшифрованных байта в начале).
pub const WIDE_STRIDE: usize = 20;
/// Допуск сверки bbox при подборе раскладки, метры.
pub const BBOX_TOLERANCE: f64 = 2e-3;

/// Точка облака: координаты в метрах, цвет RGB, интенсивность.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X, метры.
    pub x: f64,
    /// Y, метры.
    pub y: f64,
    /// Z, метры.
    pub z: f64,
    /// Красный канал.
    pub r: u8,
    /// Зелёный канал.
    pub g: u8,
    /// Синий канал.
    pub b: u8,
    /// Интенсивность (`[8]`).
    pub intensity: u8,
    /// Нерасшифрованные байты `[6], [7], [12..16]` — переносятся как есть.
    pub extra: [u8; 6],
}

impl Point {
    /// Координаты точкой (локальные, без центра).
    pub fn xyz(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// Мировые координаты: локальные + `header.center` (как `rcs2pts.py`;
    /// флаг `--local` в Python-версии эти координаты и печатает).
    pub fn world(&self, center: [f64; 3]) -> [f64; 3] {
        [
            self.x + center[0],
            self.y + center[1],
            self.z + center[2],
        ]
    }
}

/// Декодировать 16-байтовое тело точки.
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

/// Собрать 16-байтовое тело точки (обратная операция к [`decode_body`]).
///
/// Квантование: `round((coord − cube_min) × 1000 × scale)`. Если значение не
/// помещается в `u16` — ошибка [`Error::PointOutOfCube`]: точка вне куба узла.
pub fn encode_body(point: &Point, cube_min: [f64; 3]) -> Result<[u8; BODY_SIZE]> {
    let mut out = [0u8; BODY_SIZE];
    for axis in 0..3 {
        let value =
            (point.xyz()[axis] - cube_min[axis]) * 1000.0 * AXIS_SCALE[axis];
        let quantized = value.round();
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

/// Декодировать все точки узла, подбирая размер записи по bbox из записи узла.
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

/// Декодировать все узлы контейнера.
pub fn decode_all(container: &Container<'_>) -> Result<Vec<Point>> {
    let mut points = Vec::new();
    for tile in crate::tile::iter_tiles(container)? {
        points.extend(decode_tile(container, &tile)?);
    }
    Ok(points)
}

/// Совпадает ли bbox точек с bbox из записи узла (допуск `BBOX_TOLERANCE`).
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
