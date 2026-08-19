//! COBS — spec §6.1, PR-01. Delimitador 0x00, overhead peor caso 1/254.
//!
//! Convención fijada por el ancla `cobs-254` (protocol/README.md): el encoder
//! emite el grupo final vacío (código 0x01) cuando el dato termina exactamente
//! en un bloque lleno de 254 no-ceros; el decoder acepta ambas variantes.

use crate::error::DecodeError;

/// Codifica `data` con COBS. La salida no contiene ningún 0x00 y **no**
/// incluye el delimitador de frame.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 2);
    let mut code_idx = 0usize;
    out.push(0);
    let mut code: u8 = 1;
    for &b in data {
        if b != 0 {
            out.push(b);
            code += 1;
        }
        if b == 0 || code == 0xFF {
            out[code_idx] = code;
            code_idx = out.len();
            out.push(0);
            code = 1;
        }
    }
    out[code_idx] = code;
    out
}

/// Decodifica un bloque COBS (sin el delimitador 0x00).
pub fn decode(enc: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if enc.is_empty() {
        return Err(DecodeError::CobsUnderrun);
    }
    let mut out = Vec::with_capacity(enc.len());
    let mut i = 0usize;
    while i < enc.len() {
        let code = enc[i];
        if code == 0 {
            return Err(DecodeError::CobsInvalid);
        }
        i += 1;
        let n = (code - 1) as usize;
        let end = i + n;
        if end > enc.len() {
            return Err(DecodeError::CobsUnderrun);
        }
        // El delimitador no puede aparecer en ninguna posición: el splitter
        // de frames ya cortó en 0x00, así que un 0x00 acá es corrupción.
        if enc[i..end].contains(&0x00) {
            return Err(DecodeError::CobsInvalid);
        }
        out.extend_from_slice(&enc[i..end]);
        i = end;
        if code != 0xFF && i < enc.len() {
            out.push(0);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use proptest::prelude::*;

    // Anclas de protocol/README.md — el árbitro.
    #[test]
    fn ancla_cobs_empty() {
        assert_eq!(encode(&[]), [0x01]);
        assert_eq!(decode(&[0x01]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn ancla_cobs_zero_mid() {
        let dec = [0x11, 0x00, 0x22, 0x33];
        let enc = [0x02, 0x11, 0x03, 0x22, 0x33];
        assert_eq!(encode(&dec), enc);
        assert_eq!(decode(&enc).unwrap(), dec);
    }

    #[test]
    fn ancla_cobs_254() {
        let dec = vec![0x41u8; 254];
        let mut enc = vec![0xFF];
        enc.extend_from_slice(&dec);
        enc.push(0x01);
        assert_eq!(encode(&dec), enc);
        assert_eq!(decode(&enc).unwrap(), dec);
        // el decoder también acepta la variante sin grupo final vacío
        assert_eq!(decode(&enc[..enc.len() - 1]).unwrap(), dec);
    }

    #[test]
    fn rechaza_cero_interno() {
        assert_eq!(decode(&[0x02, 0x00]), Err(DecodeError::CobsInvalid));
    }

    #[test]
    fn rechaza_underrun() {
        assert_eq!(decode(&[]), Err(DecodeError::CobsUnderrun));
        assert_eq!(decode(&[0x05, 0x11]), Err(DecodeError::CobsUnderrun));
    }

    proptest! {
        #[test]
        fn ida_y_vuelta(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let enc = encode(&data);
            prop_assert!(!enc.contains(&0x00), "la salida COBS no puede contener 0x00");
            prop_assert_eq!(decode(&enc).unwrap(), data);
        }
    }
}
