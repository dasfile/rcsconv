//! Reading `.pts` (Leica Cyclone text): `x y z [intensity] r g b`.
//!
//! Port of `tools/pts_read.py`; the input format is described in
//! `docs/RCS-FORMAT.md`, Appendix D. The first line may be the point count (then
//! it is not parsed as coordinates and is simply skipped); there are 3..7
//! columns and intensity is optional.
//!
//! Point order is preserved as in the file — this matters for byte parity with
//! the Python reference.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::io::BufRead;
use std::path::PathBuf;

use rcs_cloud::progress::{NoProgress, Progress, Stage};
use rcs_cloud::{Bounds, CloudPoint, Error, Result, Source};

/// How many lines to parse between progress reports.
const REPORT_LINES: usize = 1 << 16;

/// `.pts` point source.
pub struct PtsSource {
    path: PathBuf,
    cache: RefCell<Option<Vec<CloudPoint>>>,
}

impl PtsSource {
    /// Source for a path.
    pub fn new(path: impl Into<PathBuf>) -> PtsSource {
        PtsSource { path: path.into(), cache: RefCell::new(None) }
    }

    /// Read the file once (reporting progress by bytes).
    fn ensure_loaded(&self, progress: &dyn Progress) -> Result<()> {
        if self.cache.borrow().is_some() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&self.path)?;
        let total = text.len() as u64;
        let mut points = Vec::new();
        let mut offset = 0u64;
        progress.stage(Stage::Read, 0, total);
        for (index, line) in text.lines().enumerate() {
            offset += line.len() as u64 + 1;
            if let Some(point) = parse_line(line) {
                points.push(point);
            }
            if index % REPORT_LINES == REPORT_LINES - 1 {
                progress.stage(Stage::Read, offset.min(total), total);
                if progress.cancelled() {
                    return Err(Error::Cancelled);
                }
            }
        }
        progress.stage(Stage::Read, total, total);
        *self.cache.borrow_mut() = Some(points);
        Ok(())
    }

    /// Take the loaded points out of the cache.
    fn take(&self) -> Vec<CloudPoint> {
        self.cache.borrow_mut().take().unwrap_or_default()
    }
}

/// Parse a `.pts` line; `None` means the line is not a point (header, garbage).
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

/// Clamp a value to 0..=255.
fn clamp8(value: i64) -> u8 {
    value.clamp(0, 255) as u8
}

impl Source for PtsSource {
    fn bounds_hint(&self) -> Option<Bounds> {
        None
    }

    fn scan(
        &mut self,
        progress: &dyn Progress,
        visit: &mut dyn FnMut(CloudPoint) -> Result<()>,
    ) -> Result<()> {
        let file = std::fs::File::open(&self.path)?;
        let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let reader = std::io::BufReader::new(file);
        let mut offset = 0u64;
        progress.stage(Stage::Read, 0, total);
        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            offset += line.len() as u64 + 1;
            if let Some(point) = parse_line(&line) {
                visit(point)?;
            }
            if index % REPORT_LINES == REPORT_LINES - 1 {
                progress.stage(Stage::Read, offset.min(total), total);
                if progress.cancelled() {
                    return Err(Error::Cancelled);
                }
            }
        }
        progress.stage(Stage::Read, total, total);
        Ok(())
    }

    fn bounds(&self) -> Result<Bounds> {
        self.bounds_with(&NoProgress)
    }

    fn bounds_with(&self, progress: &dyn Progress) -> Result<Bounds> {
        self.ensure_loaded(progress)?;
        let cache = self.cache.borrow();
        let points = cache.as_ref().expect("file has been read");
        if points.is_empty() {
            return Err(Error::Source(format!(
                "no points in {}",
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
        self.points_with(&NoProgress)
    }

    fn points_with(&mut self, progress: &dyn Progress) -> Result<Vec<CloudPoint>> {
        self.ensure_loaded(progress)?;
        Ok(self.take())
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
        let point = parse_line("1.5 2.5 -3.5").expect("point");
        assert_eq!(point.xyz(), [1.5, 2.5, -3.5]);
        assert_eq!((point.intensity, point.r, point.g, point.b), (0, 0, 0, 0));
    }

    #[test]
    fn line_with_intensity_and_color() {
        let point = parse_line("-0.5 0 1 42 10 20 30").expect("point");
        assert_eq!((point.intensity, point.r, point.g, point.b), (42, 10, 20, 30));
    }

    #[test]
    fn line_with_color_only() {
        let point = parse_line("0 0 0 255 1 2").expect("point");
        assert_eq!((point.intensity, point.r, point.g, point.b), (0, 255, 1, 2));
    }

    #[test]
    fn header_line_is_skipped() {
        assert!(parse_line("324680").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn values_are_clamped() {
        let point = parse_line("0 0 0 300 -5 1000 7").expect("point");
        assert_eq!((point.intensity, point.r, point.g, point.b), (255, 0, 255, 7));
    }
}
