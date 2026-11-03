//! Чтение LAS/LAZ как источника точек для `.rcs`.
//!
//! Порт `tools/las_read.py`. Декомпрессия LAZ — крейт `laz` (чистый Rust, без
//! внешних DLL): поэтому `rcsconv.exe` остаётся одним файлом (`docs/ROADMAP.md` §2).
//!
//! Правила формата `.rcs` поверх LAS: 16-битные цвет и интенсивность приводятся
//! к 8 битам (`auto` = ÷257, если максимум по файлу > 255), координаты можно
//! масштабировать (`--units-scale`) и сдвигать (`--recenter`).
//!
//! ПОДТВЕРЖДЕНО (паритет с Python): `bounds()` берёт bbox **из заголовка LAS**
//! (а не из точек), `count` — `number_of_points`; максимумы цвета и
//! интенсивности считаются по **всему файлу**, как `laspy.chunk_iterator` в
//! эталоне, хотя в коде Python параметр называется `sample`.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use rcs_cloud::{Bounds, CloudPoint, Error, Result, Source};

/// Откуда вычитать координаты при `--recenter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recenter {
    /// Не сдвигать.
    None,
    /// Вычесть центр bbox.
    Center,
    /// Вычесть минимум bbox.
    Min,
}

impl Recenter {
    /// Разобрать значение аргумента.
    pub fn parse(text: &str) -> Option<Recenter> {
        match text {
            "none" => Some(Recenter::None),
            "center" => Some(Recenter::Center),
            "min" => Some(Recenter::Min),
            _ => None,
        }
    }
}

/// Как переводить 16-битные атрибуты LAS в 8-битные `.rcs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// ÷257, если максимум по файлу > 255, иначе 1.
    Auto,
    /// Всегда ÷257.
    Div257,
    /// Всегда 1 (значения уже 0…255).
    One,
}

impl ScaleMode {
    /// Разобрать значение аргумента.
    pub fn parse(text: &str) -> Option<ScaleMode> {
        match text {
            "auto" => Some(ScaleMode::Auto),
            "257" => Some(ScaleMode::Div257),
            "1" => Some(ScaleMode::One),
            _ => None,
        }
    }

    /// Множитель для атрибута с данным максимумом.
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

/// Как превращать LAS в точки `.rcs`.
#[derive(Debug, Clone)]
pub struct LasOptions {
    /// Множитель координат (футы → метры: 0.3048).
    pub units: f64,
    /// Сдвиг координат.
    pub recenter: Recenter,
    /// Читать цвет.
    pub color: bool,
    /// Читать интенсивность.
    pub intensity: bool,
    /// Режим цвета.
    pub color_scale: ScaleMode,
    /// Режим интенсивности.
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

/// Источник точек LAS/LAZ.
///
/// Файл читается **один раз**: `bounds()` и `points()` берут готовые значения
/// из кеша (`points()` забирает вектор, повторный вызов перечитывает файл).
pub struct LasSource {
    path: PathBuf,
    options: LasOptions,
    cache: RefCell<Option<Loaded>>,
}

/// Прочитанное содержимое файла.
struct Loaded {
    count: u64,
    low: [f64; 3],
    high: [f64; 3],
    points: Vec<CloudPoint>,
}

impl LasSource {
    /// Источник для пути с настройками чтения.
    pub fn new(path: impl Into<PathBuf>, options: LasOptions) -> LasSource {
        LasSource { path: path.into(), options, cache: RefCell::new(None) }
    }

    /// Прочитать и закешировать файл (повторный вызов — без чтения).
    fn ensure_loaded(&self) -> Result<()> {
        if self.cache.borrow().is_some() {
            return Ok(());
        }
        let mut reader = las::Reader::from_path(&self.path).map_err(map_error)?;
        let (count, low, high, has_color) = {
            let header = reader.header();
            let bounds = header.bounds();
            (
                header.number_of_points(),
                [bounds.min.x, bounds.min.y, bounds.min.z],
                [bounds.max.x, bounds.max.y, bounds.max.z],
                header.point_format().has_color,
            )
        };

        let mut raw = Vec::with_capacity(count as usize);
        reader
            .read_all_points_into(&mut raw)
            .map_err(map_error)?;

        // Максимумы — по всему файлу: так же считает эталон (`chunk_iterator`).
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

        *self.cache.borrow_mut() = Some(Loaded { count, low, high, points });
        Ok(())
    }
}

/// Ошибка `las` → ошибка облака.
fn map_error(err: las::Error) -> Error {
    Error::Source(format!("LAS/LAZ: {err}"))
}

/// 16-битное значение атрибута → 0…255 (округление как в Python).
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
        self.ensure_loaded()?;
        let cache = self.cache.borrow();
        let loaded = cache.as_ref().expect("файл прочитан");
        Ok(Bounds { low: loaded.low, high: loaded.high, count: loaded.count })
    }

    fn points(&mut self) -> Result<Vec<CloudPoint>> {
        self.ensure_loaded()?;
        let mut cache = self.cache.borrow_mut();
        let loaded = cache.as_mut().expect("файл прочитан");
        Ok(loaded.points.drain(..).collect())
    }

    fn describe(&self) -> String {
        match self.path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => self.path.display().to_string(),
        }
    }
}

