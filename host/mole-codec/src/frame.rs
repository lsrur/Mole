//! Frames — spec §6.2 (PR-02..PR-06) y capa de framing COBS (§6.1).

use crate::cobs;
use crate::crc32::crc32;
use crate::error::DecodeError;
use crate::rw::Reader;
use crate::wire::{FRAME_HEADER_LEN, FRAME_MAX, RECORD_HEADER_LEN};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub ver: u8,
    /// bits 4-7 del `ver_flags`, ya desplazados al nibble bajo.
    pub flags: u8,
    pub seq: u16,
    pub t_base_us: u64,
}

/// Record sin decodificar: header PR-07 + payload crudo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub rtype: u8,
    pub dt_us: u16,
    pub payload: Vec<u8>,
}

/// Arma un frame a partir de records ya codificados (header PR-07 incluido).
/// Devuelve `(pre_cobs, wire)`; `wire` incluye el delimitador 0x00 final.
pub fn encode_frame(
    seq: u16,
    t_base_us: u64,
    flags: u8,
    encoded_records: &[Vec<u8>],
) -> Result<(Vec<u8>, Vec<u8>), DecodeError> {
    if encoded_records.len() > u16::MAX as usize {
        return Err(DecodeError::LenOverflow);
    }
    let ver_flags = 2u8 | (flags << 4);
    let mut body = Vec::with_capacity(FRAME_HEADER_LEN + 64);
    body.push(ver_flags);
    body.extend_from_slice(&seq.to_le_bytes());
    body.extend_from_slice(&t_base_us.to_le_bytes());
    body.extend_from_slice(&(encoded_records.len() as u16).to_le_bytes());
    for r in encoded_records {
        body.extend_from_slice(r);
    }
    if body.len() + 4 > FRAME_MAX {
        return Err(DecodeError::LenOverflow);
    }
    let crc = crc32(&body);
    body.extend_from_slice(&crc.to_le_bytes());
    let mut wire = cobs::encode(&body);
    wire.push(0x00);
    Ok((body, wire))
}

/// Decodifica un frame pre-COBS: verifica CRC y separa los records.
pub fn decode_frame(pre_cobs: &[u8]) -> Result<(FrameHeader, Vec<RawRecord>), DecodeError> {
    if pre_cobs.len() < FRAME_HEADER_LEN + 4 {
        return Err(DecodeError::Truncated);
    }
    let (body, crc_bytes) = pre_cobs.split_at(pre_cobs.len() - 4);
    let stored = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
    if crc32(body) != stored {
        return Err(DecodeError::CrcMismatch);
    }
    let mut r = Reader::new(body);
    let ver_flags = r.u8()?;
    let ver = ver_flags & 0x0F;
    if ver != 2 {
        return Err(DecodeError::BadVersion);
    }
    let header = FrameHeader {
        ver,
        flags: ver_flags >> 4,
        seq: r.u16()?,
        t_base_us: r.u64()?,
    };
    let rec_count = r.u16()? as usize;
    let mut records = Vec::with_capacity(rec_count.min(1024));
    for _ in 0..rec_count {
        if r.remaining() < RECORD_HEADER_LEN {
            return Err(DecodeError::LenOverflow);
        }
        let rtype = r.u8()?;
        let len = r.u8()? as usize;
        let dt_us = r.u16()?;
        if r.remaining() < len {
            return Err(DecodeError::LenOverflow);
        }
        let payload = r.bytes(len)?.to_vec();
        records.push(RawRecord { rtype, dt_us, payload });
    }
    // rec_count es autoridad: sobras después del último record son corrupción
    if !r.is_empty() {
        return Err(DecodeError::LenOverflow);
    }
    Ok((header, records))
}

/// Decodifica un bloque de wire (COBS, sin el delimitador 0x00).
pub fn decode_wire(block: &[u8]) -> Result<(FrameHeader, Vec<RawRecord>), DecodeError> {
    let pre_cobs = cobs::decode(block)?;
    decode_frame(&pre_cobs)
}

/// Detección de huecos de `seq` — PR-06. Distingue pérdida de transporte de
/// la pérdida por backpressure del MCU (que viaja en `REC_STATS`).
#[derive(Debug, Default)]
pub struct SeqTracker {
    last: Option<u16>,
}

impl SeqTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Devuelve cuántos frames faltaron antes de este `seq` (0 = ninguno).
    pub fn observe(&mut self, seq: u16) -> u16 {
        let gap = match self.last {
            None => 0,
            Some(last) => seq.wrapping_sub(last).wrapping_sub(1),
        };
        self.last = Some(seq);
        gap
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    // Ancla frame-two-records: span-end(dt 250) + watch(dt 10), seq 1, t_base 1e6.
    const REC_SPAN_END: &str = "5102fa000700";
    const REC_WATCH: &str = "20070a000500090000c03f";
    const PRE_COBS: &str =
        "02010040420f000000000002005102fa00070020070a000500090000c03f53257235";
    const WIRE: &str =
        "0302010440420f010101010202045102fa020704 20070a020502090107c03f5325723500";

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn ancla_frame_encode() {
        let recs = vec![unhex(REC_SPAN_END), unhex(REC_WATCH)];
        let (pre, wire) = encode_frame(1, 1_000_000, 0, &recs).unwrap();
        assert_eq!(hex(&pre), PRE_COBS);
        assert_eq!(hex(&wire), hex(&unhex(WIRE)));
    }

    #[test]
    fn ancla_frame_decode() {
        let wire = unhex(WIRE);
        let (h, recs) = decode_wire(&wire[..wire.len() - 1]).unwrap();
        assert_eq!(h, FrameHeader { ver: 2, flags: 0, seq: 1, t_base_us: 1_000_000 });
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].rtype, 0x51);
        assert_eq!(recs[0].dt_us, 250);
        assert_eq!(recs[1].rtype, 0x20);
        assert_eq!(hex(&recs[1].payload), "0500090000c03f");
    }

    #[test]
    fn crc_alterado_un_bit() {
        let mut pre = unhex(PRE_COBS);
        pre[5] ^= 0x01;
        assert_eq!(decode_frame(&pre), Err(DecodeError::CrcMismatch));
    }

    #[test]
    fn frame_truncado() {
        let pre = unhex(PRE_COBS);
        assert_eq!(decode_frame(&pre[..10]), Err(DecodeError::Truncated));
    }

    #[test]
    fn len_de_record_excede_frame() {
        // frame válido con un record cuyo len promete más de lo que hay
        let rec = vec![0x51u8, 0xFF, 0x00, 0x00, 0x07, 0x00]; // len=255, hay 2
        let mut body = vec![0x02u8];
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&rec);
        let crc = crate::crc32::crc32(&body);
        body.extend_from_slice(&crc.to_le_bytes());
        assert_eq!(decode_frame(&body), Err(DecodeError::LenOverflow));
    }

    #[test]
    fn version_desconocida() {
        let mut body = vec![0x03u8]; // ver 3
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        let crc = crate::crc32::crc32(&body);
        body.extend_from_slice(&crc.to_le_bytes());
        assert_eq!(decode_frame(&body), Err(DecodeError::BadVersion));
    }

    #[test]
    fn frame_vacio_es_valido() {
        let (pre, _) = encode_frame(0, 0, 0, &[]).unwrap();
        let (h, recs) = decode_frame(&pre).unwrap();
        assert_eq!(h.seq, 0);
        assert!(recs.is_empty());
    }

    #[test]
    fn seq_tracker_detecta_huecos_y_wrap() {
        let mut t = SeqTracker::new();
        assert_eq!(t.observe(0), 0);
        assert_eq!(t.observe(1), 0);
        assert_eq!(t.observe(5), 3); // faltaron 2,3,4
        let mut t = SeqTracker::new();
        assert_eq!(t.observe(65535), 0);
        assert_eq!(t.observe(0), 0); // wrap limpio
        assert_eq!(t.observe(2), 1); // faltó el 1
    }

    #[test]
    fn flags_van_y_vuelven() {
        let (pre, _) = encode_frame(9, 42, 0b0000_0101, &[]).unwrap();
        let (h, _) = decode_frame(&pre).unwrap();
        assert_eq!(h.flags, 0b0000_0101); // CATALOG + PAUSED
    }
}
