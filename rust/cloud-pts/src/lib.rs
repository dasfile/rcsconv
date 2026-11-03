//! Чтение `.pts` (текст Leica Cyclone): `x y z [intensity] r g b`.
//!
//! Порт `tools/pts_read.py`; формат входа — `docs/RCS-FORMAT.md`, Приложение D.
//! Первая строка может быть числом точек (тогда она не разберётся как строка
//! координат и просто пропускается), колонок 3…7, интенсивность необязательна.
//!
//! Порядок точек сохраняется как в файле — это важно для побайтового паритета
//! с Python-эталоном.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use rcs_cloud::{Bounds, CloudPoint, Error, Result, Source};

/// Источник точек `.pts`.
pub struct PtsSource {
    path: PathBuf,
    cache: RefCell<Option<Vec<CloudPoint>>>,
}

impl PtsSource {
    /// Источник для пути.
    pub fn new(path: impl Into<PathBuf>) -> PtsSource {
        PtsSource { path: path.into(), cache: RefCell::new(None) }
    }

    /// Прочитать файл один раз.
    fn ensure_loaded(&self) -> Result<()> {
        if self.cache.borrow().is_some() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&self.path)?;
        let mut points = Vec::new();
        for line in text.lines() {
            if let Some(point) = parse_line(line) {
                points.push(point);
            }
        }
        *self.cache.borrow_mut() = Some(points);
        Ok(())
    }
}

/// Разобрать строку `.pts`; `None` — строка не является точкой (шапка, мусор).
fn parse_line(line: &str) -> Option<CloudPoint> {
    let mut parts = line.split_whitespace();
    let mut coord = [0.0f64; 3];
    for slot in coord.iter_mut() {
        *slot = parts.next()?.parse().ok()?;
    }

    let numbers: Vec<i64> = parts
        .map(|token| match token.parse::<f64>() {
            Ok(value) => rcs_cloud::round_half_even(value) as i64,
            Err(_) => 0,
        })
        .collect();
    let (intensity, r, g, b) = match numbers.len() {
        0 => (0, 0, 0, 0),
        1 | 2 => (numbers[0], 0, 0, 0),
        3 => (0, numbers[0], numbers[1], numbers[2]),
        _ => (numbers[0], numbers[1], numbers[2], numbers[3]),
    };
    Some(CloudPoint::new(
        coord[0],
        coord[1],
        coord[2],
        clamp8(intensity),
        clamp8(r),
        clamp8(g),
        clamp8(b),
    ))
}

/// Привести значение к 0…255.
fn clamp8(value: i64) -> u8 {
    value.clamp(0, 255) as u8
}

impl Source for PtsSource {
    fn bounds(&self) -> Result<Bounds> {
        self.ensure_loaded()?;
        let cache = self.cache.borrow();
        let points = cache.as_ref().expect("файл прочитан");
        if points.is_empty() {
            return Err(Error::Source(format!(
                "в {} нет точек",
                self.path.display()
            )));
        }
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for point in points {
            for axis in 0..3 {
                low[axis] = low[axis].min(point.xyz()[axis]);
                high[axis] = high[axis].max(point.xyz()[axis]);
            }
        }
        Ok(Bounds { low, high, count: points.len() as u64 })
    }

    fn points(&mut self) -> Result<Vec<CloudPoint>> {
        self.ensure_loaded()?;
        let mut cache = self.cache.borrow_mut();
        Ok(cache.as_mut().expect("файл прочитан").drain(..).collect())
    }

    fn describe(&self) -> String {
        match self.path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => self.path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_with_three_columns() {
        let point = parse_line("1.5 2.5 -3.5").expect("точка");
        assert_eq!(point.xyz(), [1.5, 2.5, -3.5]);
        assert_eq!((point.intensity, point.r, point.g, point.b), (0, 0, 0, 0));
    }

    #[test]
    fn line_with_intensity_and_color() {
        let point = parse_line("-0.5 0 1 42 10 20 30").expect("точка");
        assert_eq!((point.intensity, point.r, point.g, point.b), (42, 10, 20, 30));
    }

    #[test]
    fn line_with_color_only() {
        let point = parse_line("0 0 0 255 1 2").expect("точка");
        assert_eq!((point.intensity, point.r, point.g, point.b), (0, 255, 1, 2));
    }

    #[test]
    fn header_line_is_skipped() {
        assert!(parse_line("324680").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn values_are_clamped() {
        let point = parse_line("0 0 0 300 -5 1000 7").expect("точка");
        assert_eq!((point.intensity, point.r, point.g, point.b), (255, 0, 255, 7));
    }
}
