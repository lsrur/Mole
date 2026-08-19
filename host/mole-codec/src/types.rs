//! Descriptores de tipo — REC-43 (draft.9, con `flags` por campo) y registro
//! de tipos del lado del host.

use std::collections::HashMap;

use crate::error::DecodeError;
use crate::rw::{put_str, put_u16, Reader};
use crate::wire::{WireType, FIELD_FLAG_BE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name_sym: u16,
    pub wire: WireType,
    /// bit 0 = big-endian (el MCU nunca invierte; invierte el host, REC-45).
    pub flags: u8,
    /// Offset real en memoria — SOLO para el modo bytes (Q-3).
    pub offset: u16,
    pub size: u16,
    /// `type_id` del struct anidado cuando `wire == Struct`.
    pub ref_type: u16,
}

impl FieldDef {
    pub fn is_big_endian(&self) -> bool {
        self.flags & FIELD_FLAG_BE != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    pub type_id: u16,
    pub name: Vec<u8>,
    pub fields: Vec<FieldDef>,
}

impl TypeDef {
    /// Payload de `REC_TYPE_DEF` — REC-43.
    pub fn encode_payload(&self) -> Result<Vec<u8>, DecodeError> {
        let mut out = Vec::new();
        put_u16(&mut out, self.type_id);
        put_str(&mut out, &self.name)?;
        if self.fields.len() > 255 {
            return Err(DecodeError::LenOverflow);
        }
        out.push(self.fields.len() as u8);
        for f in &self.fields {
            put_u16(&mut out, f.name_sym);
            out.push(f.wire.to_u8());
            out.push(f.flags);
            put_u16(&mut out, f.offset);
            put_u16(&mut out, f.size);
            put_u16(&mut out, f.ref_type);
        }
        Ok(out)
    }

    pub fn decode_payload(payload: &[u8]) -> Result<TypeDef, DecodeError> {
        let mut r = Reader::new(payload);
        let type_id = r.u16()?;
        let name = r.str_pr20()?.to_vec();
        let nfields = r.u8()? as usize;
        let mut fields = Vec::with_capacity(nfields.min(64));
        for _ in 0..nfields {
            let name_sym = r.u16()?;
            let wire_raw = r.u8()?;
            // Un tipo de campo desconocido invalida el descriptor entero:
            // sin él no se puede decodificar el modo valores.
            let wire = WireType::from_u8(wire_raw).ok_or(DecodeError::Truncated)?;
            let flags = r.u8()?;
            let offset = r.u16()?;
            let size = r.u16()?;
            let ref_type = r.u16()?;
            fields.push(FieldDef {
                name_sym,
                wire,
                flags,
                offset,
                size,
                ref_type,
            });
        }
        if !r.is_empty() {
            return Err(DecodeError::LenOverflow);
        }
        Ok(TypeDef {
            type_id,
            name,
            fields,
        })
    }
}

/// Registro de tipos conocidos, poblado con los `REC_TYPE_DEF` recibidos.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    map: HashMap<u16, TypeDef>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: TypeDef) {
        self.map.insert(def.type_id, def);
    }

    pub fn get(&self, type_id: u16) -> Option<&TypeDef> {
        self.map.get(&type_id)
    }
}
