//! Point of the cloud at the build input.

/// Cloud point: coordinates in metres plus 0..255 intensity and colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudPoint {
    /// X, metres (world coordinates).
    pub x: f64,
    /// Y, metres.
    pub y: f64,
    /// Z, metres.
    pub z: f64,
    /// Intensity 0..255.
    pub intensity: u8,
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl CloudPoint {
    /// Create a point.
    pub fn new(x: f64, y: f64, z: f64, intensity: u8, r: u8, g: u8, b: u8) -> CloudPoint {
        CloudPoint { x, y, z, intensity, r, g, b }
    }

    /// Coordinates as an array.
    pub fn xyz(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}
