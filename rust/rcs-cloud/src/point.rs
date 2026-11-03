//! Точка облака на входе сборки.

/// Точка облака: координаты в метрах + интенсивность и цвет 0…255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudPoint {
    /// X, метры (мировые координаты).
    pub x: f64,
    /// Y, метры.
    pub y: f64,
    /// Z, метры.
    pub z: f64,
    /// Интенсивность 0…255.
    pub intensity: u8,
    /// Красный канал.
    pub r: u8,
    /// Зелёный канал.
    pub g: u8,
    /// Синий канал.
    pub b: u8,
}

impl CloudPoint {
    /// Создать точку.
    pub fn new(x: f64, y: f64, z: f64, intensity: u8, r: u8, g: u8, b: u8) -> CloudPoint {
        CloudPoint { x, y, z, intensity, r, g, b }
    }

    /// Координаты точкой.
    pub fn xyz(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}
