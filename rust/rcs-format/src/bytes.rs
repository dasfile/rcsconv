//! Чтение и запись полей Little-endian без выравнивания.

use crate::error::{Error, Result};

/// Проверить, что в буфере есть `len` байт начиная с `offset`.
pub(crate) fn need(data: &[u8], offset: usize, len: usize) -> Result<()> {
    let end = offset
        .checked_add(len)
        .ok_or(Error::TooShort { size: data.len(), need: usize::MAX })?;
    if end > data.len() {
        return Err(Error::TooShort { size: data.len(), need: end });
    }
    Ok(())
}

/// Прочитать `u32` по смещению.
pub(crate) fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    need(data, offset, 4)?;
    Ok(u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()))
}

/// Прочитать `u64` по смещению.
pub(crate) fn u64_at(data: &[u8], offset: usize) -> Result<u64> {
    need(data, offset, 8)?;
    Ok(u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()))
}

/// Прочитать `f64` по смещению.
pub(crate) fn f64_at(data: &[u8], offset: usize) -> Result<f64> {
    need(data, offset, 8)?;
    Ok(f64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()))
}

/// Прочитать три `f64` подряд (координаты/габариты).
pub(crate) fn f64x3_at(data: &[u8], offset: usize) -> Result<[f64; 3]> {
    Ok([
        f64_at(data, offset)?,
        f64_at(data, offset + 8)?,
        f64_at(data, offset + 16)?,
    ])
}

/// Записать `u32` по смещению (буфер должен быть достаточного размера).
pub(crate) fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Записать `u64` по смещению.
pub(crate) fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Записать `f64` по смещению.
pub(crate) fn put_f64(out: &mut [u8], offset: usize, value: f64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Записать три `f64` подряд.
pub(crate) fn put_f64x3(out: &mut [u8], offset: usize, value: [f64; 3]) {
    put_f64(out, offset, value[0]);
    put_f64(out, offset + 8, value[1]);
    put_f64(out, offset + 16, value[2]);
}
