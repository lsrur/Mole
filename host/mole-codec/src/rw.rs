//! Lectura y escritura little-endian de payloads. El lector nunca paniquea:
//! todo acceso fuera de rango devuelve `Truncated` (SEC-04).

use crate::error::DecodeError;

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < n {
            return Err(DecodeError::Truncated);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Cadena con prefijo de longitud u8 — PR-20. Devuelve los bytes crudos:
    /// el UTF-8 inválido se resuelve con `from_utf8_lossy` recién al renderizar.
    pub fn str_pr20(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.u8()? as usize;
        self.bytes(n)
    }
}

pub fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Cadena con prefijo u8 (PR-20). Trunca a 255 jamás: el llamador garantiza.
pub fn put_str(out: &mut Vec<u8>, s: &[u8]) -> Result<(), DecodeError> {
    if s.len() > 255 {
        return Err(DecodeError::LenOverflow);
    }
    out.push(s.len() as u8);
    out.extend_from_slice(s);
    Ok(())
}
