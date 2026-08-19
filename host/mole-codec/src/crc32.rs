//! CRC32 — spec PR-03: "CRC32 (poly zlib)".
//!
//! Convención fijada por el ancla `crc32-canonical` (riesgo Q-1 del plan):
//! `crc = ~esp_rom_crc32_le(~0, buf, len)` equivale al CRC-32/ISO-HDLC
//! estándar: polinomio reflejado 0xEDB88320, init 0xFFFFFFFF, XOR final
//! 0xFFFFFFFF. Valor de chequeo: CRC32("123456789") = 0xCBF43926.

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

static TABLE: [u32; 256] = build_table();

/// CRC32 de un buffer completo.
pub fn crc32(data: &[u8]) -> u32 {
    update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}

/// Actualización incremental sobre el estado crudo (sin inversiones).
/// Para el uso incremental de CAT-08 ver [`Crc32`].
fn update(mut state: u32, data: &[u8]) -> u32 {
    for &b in data {
        state = TABLE[((state ^ b as u32) & 0xFF) as usize] ^ (state >> 8);
    }
    state
}

/// CRC32 incremental. Usado por el `catalog_hash` (CAT-08): el estado se
/// arrastra de una definición a la siguiente.
#[derive(Debug, Clone, Copy)]
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    pub fn new() -> Self {
        Crc32 { state: 0xFFFF_FFFF }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.state = update(self.state, data);
    }

    pub fn value(&self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancla_crc32_canonical() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn vacio_y_un_byte() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(&[0x00]), 0xD202_EF8D);
    }

    #[test]
    fn ejemplo_del_plan() {
        // mole-f0-plan.md §4: 01020304 → b63cfbcd
        assert_eq!(crc32(&[1, 2, 3, 4]), 0xB63C_FBCD);
    }

    #[test]
    fn incremental_equivale_a_completo() {
        let mut inc = Crc32::new();
        inc.feed(b"1234");
        inc.feed(b"56789");
        assert_eq!(inc.value(), crc32(b"123456789"));
    }
}
