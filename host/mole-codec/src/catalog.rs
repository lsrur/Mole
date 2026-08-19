//! Catálogo del lado del host — §6.4/§6.5. Símbolos, definiciones de formato,
//! tipos, `catalog_hash` (CAT-08) y resolución tipada de argumentos.

use std::collections::HashMap;

use crate::args::{decode_args, Value};
use crate::crc32::Crc32;
use crate::error::DecodeError;
use crate::record::{FmtDef, Record};
use crate::types::TypeRegistry;

/// Un símbolo del catálogo (CAT-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub sym_id: u16,
    pub kind: u8,
    pub parent: u16,
    pub name: Vec<u8>,
}

/// Estado de catálogo reconstruido a partir del stream. Es la contraparte
/// host del registro del firmware; el `catalog_hash` debe coincidir con el
/// que reporta `REC_SESSION` (CAT-08..CAT-10).
#[derive(Debug, Default)]
pub struct Catalog {
    syms: HashMap<u16, Symbol>,
    fmts: HashMap<u16, FmtDef>,
    pub types: TypeRegistry,
    hash: Crc32,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Aplica un record al catálogo si es una definición; devuelve `true`
    /// si lo era. El hash incremental (CAT-08) se alimenta con el payload
    /// de cada definición, en orden de llegada.
    pub fn apply(&mut self, rec: &Record) -> Result<bool, DecodeError> {
        match rec {
            Record::SymDef { sym_id, kind, parent, name } => {
                self.feed_hash(rec)?;
                self.syms.insert(
                    *sym_id,
                    Symbol { sym_id: *sym_id, kind: *kind, parent: *parent, name: name.clone() },
                );
                Ok(true)
            }
            Record::FmtDef(f) => {
                self.feed_hash(rec)?;
                self.fmts.insert(f.fmt_id, f.clone());
                Ok(true)
            }
            Record::TypeDef(t) => {
                self.feed_hash(rec)?;
                self.types.insert(t.clone());
                Ok(true)
            }
            Record::EnumDef(e) => {
                self.feed_hash(rec)?;
                self.types.insert_enum(e.clone());
                Ok(true)
            }
            Record::SchemaDef { .. } => {
                self.feed_hash(rec)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn feed_hash(&mut self, rec: &Record) -> Result<(), DecodeError> {
        let payload = rec.encode_payload(&self.types)?;
        self.hash.feed(&payload);
        Ok(())
    }

    /// `catalog_hash` acumulado (CAT-08).
    pub fn hash(&self) -> u32 {
        self.hash.value()
    }

    pub fn sym(&self, sym_id: u16) -> Option<&Symbol> {
        self.syms.get(&sym_id)
    }

    pub fn fmt(&self, fmt_id: u16) -> Option<&FmtDef> {
        self.fmts.get(&fmt_id)
    }

    pub fn sym_count(&self) -> usize {
        self.syms.len()
    }

    /// Resuelve los argumentos crudos de un `REC_LOG_FMT`/`REC_CHECK_FAIL`
    /// contra su `REC_FMT_DEF`. Si la definición nunca llegó: `UnknownFmt`.
    pub fn parse_args(&self, fmt_id: u16, args_raw: &[u8]) -> Result<Vec<Value>, DecodeError> {
        let def = self.fmts.get(&fmt_id).ok_or(DecodeError::UnknownFmt)?;
        decode_args(&def.arg_types, args_raw, &self.types)
    }
}

/// Decide si hace falta re-emitir el catálogo completo tras un handshake
/// (CAT-10): epoch distinto o hash distinto ⇒ resync.
pub fn needs_resync(
    device_epoch: u32,
    device_hash: u32,
    known_epoch: u32,
    known_hash: u32,
) -> bool {
    device_epoch != known_epoch || device_hash != known_hash
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::args::ArgType;
    use crate::wire::WireType;

    fn defs_de_ejemplo() -> Vec<Record> {
        vec![
            Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() },
            Record::SymDef { sym_id: 4, kind: 4, parent: 0, name: b"main.cpp".to_vec() },
            Record::FmtDef(FmtDef {
                fmt_id: 7,
                file_sym: 4,
                line: 42,
                arg_types: vec![ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)],
                fmt: b"crudo={} v={:.2}".to_vec(),
            }),
        ]
    }

    #[test]
    fn hash_es_determinista_y_sensible_al_orden() {
        let mut a = Catalog::new();
        let mut b = Catalog::new();
        let defs = defs_de_ejemplo();
        for d in &defs {
            a.apply(d).unwrap();
        }
        for d in defs.iter().rev() {
            b.apply(d).unwrap();
        }
        assert_ne!(a.hash(), b.hash(), "el hash debe depender del orden de emision");
        let mut c = Catalog::new();
        for d in &defs {
            c.apply(d).unwrap();
        }
        assert_eq!(a.hash(), c.hash());
    }

    #[test]
    fn resync_por_epoch_o_hash() {
        assert!(!needs_resync(10, 0xAA, 10, 0xAA));
        assert!(needs_resync(11, 0xAA, 10, 0xAA)); // reboot
        assert!(needs_resync(10, 0xBB, 10, 0xAA)); // catalogo creció
    }

    #[test]
    fn parse_args_contra_fmt_def() {
        let mut cat = Catalog::new();
        for d in defs_de_ejemplo() {
            cat.apply(&d).unwrap();
        }
        let raw = [0x00, 0x04, 0x00, 0x00, 0x7b, 0x14, 0x1e, 0x40];
        let vals = cat.parse_args(7, &raw).unwrap();
        assert_eq!(vals[0], Value::I32(1024));
        // fmt_id inexistente → UnknownFmt, nunca panic
        assert_eq!(cat.parse_args(99, &raw), Err(DecodeError::UnknownFmt));
    }

    #[test]
    fn los_no_definicion_no_tocan_el_hash() {
        let mut cat = Catalog::new();
        cat.apply(&Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"x".to_vec() })
            .unwrap();
        let h = cat.hash();
        cat.apply(&Record::SpanEnd { span_id: 1 }).unwrap();
        cat.apply(&Record::Event { sym: 1, arg: 2 }).unwrap();
        assert_eq!(cat.hash(), h);
    }
}
