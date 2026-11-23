//! Reading LAS/LAZ as a point source for `.rcs`.
//!
//! Port of `tools/las_read.py`. LAZ decompression uses the `laz` crate (pure
//! Rust, no external DLLs), so `rcsconv.exe` stays a single file
//! (`docs/ROADMAP.md` 2).
//!
//! `.rcs` rules on top of LAS: 16-bit colour and intensity are reduced to 8 bits
//! (`auto` = divide by 257 when the file maximum is > 255), coordinates can be
//! scaled (`--units-scale`) and shifted (`--recenter`).
//!
//! CONFIRMED (parity with Python): `bounds()` takes the bbox **from the LAS
//! header** (not from the points), `count` is `number_of_points`; colour and
//! intensity maxima are computed over the **whole file**, like
//! `laspy.chunk_iterator` in the reference, even though the Python parameter is
//! called `sample`.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use rcs_cloud::progress::{NoProgress, Progress, Stage};
use rcs_cloud::{Bounds, CloudPoint, Error, Result, Source};

/// What to subtract from coordinates under `--recenter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recenter {
    /// Do not shift.
    None,
    /// Subtract the bbox centre.
    Center,
    /// Subtract the bbox minimum.
    Min,
}

impl Recenter {
    /// Parse the argument value.
    pub fn parse(text: &str) -> Option<Recenter> {
        match text {
            "none" => Some(Recenter::None),
            "center" => Some(Recenter::Center),
            "min" => Some(Recenter::Min),
            _ => None,
        }
    }
}

/// How to convert 16-bit LAS attributes to 8-bit `.rcs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// Divide by 257 when the file maximum is > 255, otherwise use 1.
    Auto,
    /// Always divide by 257.
    Div257,
    /// Always 1 (values are already 0..255).
    One,
}

impl ScaleMode {
    /// Parse the argument value.
    pub fn parse(text: &str) -> Option<ScaleMode> {
        match text {
            "auto" => Some(ScaleMode::Auto),
            "257" => Some(ScaleMode::Div257),
            "1" => Some(ScaleMode::One),
            _ => None,
        }
    }

    /// Multiplier for an attribute with the given maximum.
    pub fn factor(self, maximum: u16) -> f64 {
        match self {
            ScaleMode::One => 1.0,
            ScaleMode::Div257 => 1.0 / 257.0,
            ScaleMode::Auto => {
                if maximum > 255 {
                    1.0 / 257.0
                } else {
                    1.0
                }
            }
        }
    }
}

/// How to turn LAS into `.rcs` points.
#[derive(Debug, Clone)]
pub struct LasOptions {
    /// Coordinate multiplier (feet -> metres: 0.3048).
    pub units: f64,
    /// Coordinate shift.
    pub recenter: Recenter,
    /// Read colour.
    pub color: bool,
    /// Read intensity.
    pub intensity: bool,
    /// Colour mode.
    pub color_scale: ScaleMode,
    /// Intensity mode.
    pub intensity_scale: ScaleMode,
}

impl Default for LasOptions {
    fn default() -> Self {
        LasOptions {
            units: 1.0,
            recenter: Recenter::None,
            color: true,
            intensity: true,
            color_scale: ScaleMode::Auto,
            intensity_scale: ScaleMode::Auto,
        }
    }
}

/// How many points to read per pass (with `read_points_into`).
const READ_CHUNK: u64 = 1 << 16;

/// What is taken from the LAS header: bbox, `count`, colour presence.
#[derive(Debug, Clone, Copy)]
struct HeaderInfo {
    count: u64,
    low: [f64; 3],
    high: [f64; 3],
    has_color: bool,
}

/// LAS/LAZ point source.
///
/// `bounds()` reads **only the header**, points are read on the first
/// `points()`/`points_with()` and cached (`points()` takes the vector, a second
/// call re-reads the file).
pub struct LasSource {
    path: PathBuf,
    options: LasOptions,
    header: RefCell<Option<HeaderInfo>>,
    cache: RefCell<Option<Vec<CloudPoint>>>,
}

impl LasSource {
    /// Source for a path with read settings.
    pub fn new(path: impl Into<PathBuf>, options: LasOptions) -> LasSource {
        LasSource {
            path: path.into(),
            options,
            header: RefCell::new(None),
            cache: RefCell::new(None),
        }
    }

    /// File header (read once).
    fn header_info(&self) -> Result<HeaderInfo> {
        if let Some(info) = self.header.borrow().as_ref() {
            return Ok(*info);
        }
        let reader = las::Reader::from_path(&self.path).map_err(map_error)?;
        let header = reader.header();
        let bounds = header.bounds();
        let info = HeaderInfo {
            count: header.number_of_points(),
            low: [bounds.min.x, bounds.min.y, bounds.min.z],
            high: [bounds.max.x, bounds.max.y, bounds.max.z],
            has_color: header.point_format().has_color,
        };
        *self.header.borrow_mut() = Some(info);
        Ok(info)
    }

    /// Read and cache the points (a repeated call does not read the file).
    fn ensure_loaded(&self, progress: &dyn Progress) -> Result<()> {
        if self.cache.borrow().is_some() {
            return Ok(());
        }
        let info = self.header_info()?;
        let count = info.count;
        let has_color = info.has_color;
        let low = info.low;
        let high = info.high;
        let mut reader = las::Reader::from_path(&self.path).map_err(map_error)?;

        // Read in chunks: same result as `read_all_points_into`, but it allows
        // reporting progress and cancelling.
        let mut raw: Vec<las::Point> = Vec::with_capacity(count as usize);
        progress.stage(Stage::Read, 0, count);
        loop {
            let before = raw.len() as u64;
            let read = reader
                .read_points_into(READ_CHUNK, &mut raw)
                .map_err(map_error)?;
            if read == 0 {
                break;
            }
            progress.stage(Stage::Read, before + read, count);
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
        }

        // Maxima over the whole file: the reference counts the same way (`chunk_iterator`).
        let mut color_max = 0u16;
        let mut intensity_max = 0u16;
        for point in &raw {
            intensity_max = intensity_max.max(point.intensity);
            if let Some(color) = point.color {
                color_max = color_max.max(color.red).max(color.green).max(color.blue);
            }
        }

        let take_color = self.options.color && has_color;
        let color_scale = if take_color {
            self.options.color_scale.factor(color_max)
        } else {
            1.0
        };
        let intensity_scale = if self.options.intensity {
            self.options.intensity_scale.factor(intensity_max)
        } else {
            1.0
        };
        let shift = match self.options.recenter {
            Recenter::None => [0.0; 3],
            Recenter::Center => [
                (low[0] + high[0]) / 2.0,
                (low[1] + high[1]) / 2.0,
                (low[2] + high[2]) / 2.0,
            ],
            Recenter::Min => low,
        };

        let units = self.options.units;
        let points = raw
            .iter()
            .map(|point| {
                let (r, g, b) = match point.color {
                    Some(color) if take_color => (
                        to8(color.red, color_scale),
                        to8(color.green, color_scale),
                        to8(color.blue, color_scale),
                    ),
                    _ => (0, 0, 0),
                };
                CloudPoint::new(
                    (point.x - shift[0]) * units,
                    (point.y - shift[1]) * units,
                    (point.z - shift[2]) * units,
                    to8(point.intensity, intensity_scale),
                    r,
                    g,
                    b,
                )
            })
            .collect();

        *self.cache.borrow_mut() = Some(points);
        Ok(())
    }

    /// Take the loaded points out of the cache.
    fn take(&self) -> Vec<CloudPoint> {
        self.cache.borrow_mut().take().unwrap_or_default()
    }
}

/// `las` error -> cloud error.
fn map_error(err: las::Error) -> Error {
    Error::Source(format!("LAS/LAZ: {err}"))
}

/// 16-bit attribute value -> 0..=255 (rounding as in Python).
fn to8(value: u16, scale: f64) -> u8 {
    let scaled = rcs_cloud::round_half_even(value as f64 * scale);
    if scaled <= 0.0 {
        0
    } else if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

impl Source for LasSource {
    fn bounds(&self) -> Result<Bounds> {
        self.bounds_with(&NoProgress)
    }

    fn bounds_with(&self, _progress: &dyn Progress) -> Result<Bounds> {
        let info = self.header_info()?;
        Ok(Bounds { low: info.low, high: info.high, count: info.count })
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

