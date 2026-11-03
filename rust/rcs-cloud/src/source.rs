//! bbox и контракт источника точек.

use crate::error::Result;
use crate::point::CloudPoint;

/// bbox и число точек источника.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Минимум bbox.
    pub low: [f64; 3],
    /// Максимум bbox.
    pub high: [f64; 3],
    /// Число точек.
    pub count: u64,
}

impl Bounds {
    /// Центр bbox — он становится `center` заголовка.
    pub fn center(&self) -> [f64; 3] {
        [
            (self.low[0] + self.high[0]) / 2.0,
            (self.low[1] + self.high[1]) / 2.0,
            (self.low[2] + self.high[2]) / 2.0,
        ]
    }

    /// Полупротяжённость bbox — она становится `extent` заголовка.
    pub fn half_extent(&self) -> [f64; 3] {
        [
            (self.high[0] - self.low[0]) / 2.0,
            (self.high[1] - self.low[1]) / 2.0,
            (self.high[2] - self.low[2]) / 2.0,
        ]
    }
}

/// Источник точек для сборки `.rcs`.
///
/// Контракт повторяет `cloud.CloudSource`: сначала `bounds()`, затем `points()`.
/// В Python `points()` — генератор на несколько проходов; в Rust точки читаются
/// одним вектором (на 4 млн точек это ~128 МБ, зато проход один и порядок
/// гарантирован — важно для побайтового паритета).
pub trait Source {
    /// bbox и число точек.
    fn bounds(&self) -> Result<Bounds>;

    /// Все точки в порядке файла (забирает вектор из источника).
    fn points(&mut self) -> Result<Vec<CloudPoint>>;

    /// Что за источник (для отчёта в CLI).
    fn describe(&self) -> String;
}
