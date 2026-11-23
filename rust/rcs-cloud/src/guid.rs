//! GUID generation in the ReCap format: `{8_4_4_4_12}`, lower case.
//!
//! No external dependencies: 16 random bytes come from xorshift64* seeded with
//! the time, the process id and a call counter. That is enough for a file name
//! (the GUID does not affect geometry), and determinism is not needed: the
//! reference files are compared with the GUID ignored.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generator state: shift and multiply (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// A new GUID in the ReCap format (`{8_4_4_4_12}`).
pub fn new_guid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos() as u64)
        .unwrap_or(0);
    let seed = nanos
        ^ (std::process::id() as u64) << 32
        ^ COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut rng = Rng(seed | 1);

    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let value = rng.next_u64().to_le_bytes();
        chunk.copy_from_slice(&value[..chunk.len()]);
    }
    // Version 4 and the RFC 4122 variant — same as uuid4 in the Python reference.
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{{{}_{}_{}_{}_{}}}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcs_format::header::normalize_guid;

    #[test]
    fn guid_is_acceptable_for_recap() {
        let guid = new_guid();
        assert_eq!(guid.len(), 38, "GUID string length");
        assert_eq!(normalize_guid(&guid).expect("GUID format"), guid);
        assert_ne!(new_guid(), guid, "GUIDs do not repeat");
    }
}
