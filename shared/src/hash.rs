use std::hash::Hasher;

/// FNV-1a, with every integer written little-endian at a fixed width: a
/// digest must come out the same on the aarch64 server and the x86-64 and
/// wasm32 clients, where `usize` differs.
pub(crate) struct Fnv(u64);

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = Fnv::new();
    h.write(bytes);
    h.finish()
}

impl Fnv {
    pub(crate) const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_u16(&mut self, v: u16) {
        self.write(&v.to_le_bytes());
    }

    fn write_u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }

    fn write_u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    fn write_i32(&mut self, v: i32) {
        self.write(&v.to_le_bytes());
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn write_isize(&mut self, v: isize) {
        self.write(&(v as i64).to_le_bytes());
    }
}
