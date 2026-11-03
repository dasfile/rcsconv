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

use rcs_cloud::progress::{NoProgress, Progress, Stage};
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

/// Сколько точек читать за один заход (пакеты `read_points_into`).
const READ_CHUNK: u64 = 1 << 16;

/// Что берётся из заголовка LAS: bbox, `count`, наличие цвета.
#[derive(Debug, Clone, Copy)]
struct HeaderInfo {
    count: u64,
    low: [f64; 3],
    high: [f64; 3],
    has_color: bool,
}

/// Источник точек LAS/LAZ.
///
/// `bounds()` читает **только заголовок**, точки — при первом `points()`/`points_with()`
/// и кешируются (`points()` забирает вектор, повторный вызов перечитывает файл).
pub struct LasSource {
    path: PathBuf,
    options: LasOptions,
    header: RefCell<Option<HeaderInfo>>,
    cache: RefCell<Option<Vec<CloudPoint>>>,
}

impl LasSource {
    /// Источник для пути с настройками чтения.
    pub fn new(path: impl Into<PathBuf>, options: LasOptions) -> LasSource {
        LasSource {
            path: path.into(),
            options,
            header: RefCell::new(None),
            cache: RefCell::new(None),
        }
    }

    /// Заголовок файла (читается один раз).
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

    /// Прочитать и закешировать точки (повторный вызов — без чтения).
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

        // Читаем пакетами: результат тот же, что у `read_all_points_into`,
        // но появляется возможность отчитываться о прогрессе и отменять.
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

        *self.cache.borrow_mut() = Some(points);
        Ok(())
    }

    /// Забрать прочитанные точки из кеша.
    fn take(&self) -> Vec<CloudPoint> {
        self.cache.borrow_mut().take().unwrap_or_default()
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

