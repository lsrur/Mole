//! Descriptores de tipo — REC-43 (draft.9, con `flags` por campo) y registro
//! de tipos del lado del host.

use std::collections::HashMap;

use crate::error::DecodeError;
use crate::rw::{put_str, put_u16, Reader};
use crate::wire::{WireType, FIELD_FLAG_ARRAY, FIELD_FLAG_BE, FIELD_FLAG_ENUM};

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

    /// REC-54 (draft.11): campo arreglo, solo elementos escalares.
    pub fn is_array(&self) -> bool {
        self.flags & FIELD_FLAG_ARRAY != 0
    }

    /// REC-53 (draft.11): campo enum; `ref_type` es el type_id del enum.
    pub fn is_enum(&self) -> bool {
        self.flags & FIELD_FLAG_ENUM != 0
    }

    /// Cantidad de elementos de un campo arreglo: `size / tamaño(wire)`.
    /// Un size no divisible o cero invalida el descriptor (REC-54).
    pub fn array_count(&self) -> Result<usize, DecodeError> {
        let elem = self.wire.fixed_size().ok_or(DecodeError::Truncated)?;
        let size = self.size as usize;
        if size == 0 || size % elem != 0 {
            return Err(DecodeError::Truncated);
        }
        Ok(size / elem)
    }
}

/// Descriptor de enum — REC-53 (draft.11). Comparte el espacio de `type_id`
/// con los structs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub type_id: u16,
    pub name: Vec<u8>,
    /// Entero base (REC-52); dimensiona los `value` de las entradas.
    pub wire: WireType,
    pub entries: Vec<(i64, u16)>, // (valor, name_sym)
}

impl EnumDef {
    pub fn encode_payload(&self) -> Result<Vec<u8>, DecodeError> {
        let elem = self.wire.fixed_size().ok_or(DecodeError::LenOverflow)?;
        let mut out = Vec::new();
        put_u16(&mut out, self.type_id);
        put_str(&mut out, &self.name)?;
        if self.entries.len() > 255 {
            return Err(DecodeError::LenOverflow);
        }
        out.push(self.wire.to_u8());
        out.push(self.entries.len() as u8);
        for (value, name_sym) in &self.entries {
            let bytes = value.to_le_bytes();
            out.extend_from_slice(&bytes[..elem]);
            put_u16(&mut out, *name_sym);
        }
        Ok(out)
    }

    pub fn decode_payload(payload: &[u8]) -> Result<EnumDef, DecodeError> {
        let mut r = Reader::new(payload);
        let type_id = r.u16()?;
        let name = r.str_pr20()?.to_vec();
        let wire_raw = r.u8()?;
        let wire = WireType::from_u8(wire_raw).ok_or(DecodeError::Truncated)?;
        let elem = wire.fixed_size().ok_or(DecodeError::Truncated)?;
        let signed = matches!(
            wire,
            WireType::I8 | WireType::I16 | WireType::I32 | WireType::I64
        );
        let nentries = r.u8()? as usize;
        let mut entries = Vec::with_capacity(nentries.min(64));
        for _ in 0..nentries {
            let raw = r.bytes(elem)?;
            let mut buf = [0u8; 8];
            buf[..elem].copy_from_slice(raw);
            let mut value = i64::from_le_bytes(buf);
            if signed && elem < 8 {
                // extensión de signo del entero base
                let shift = (8 - elem) * 8;
                value = (value << shift) >> shift;
            }
            let name_sym = r.u16()?;
            entries.push((value, name_sym));
        }
        if !r.is_empty() {
            return Err(DecodeError::LenOverflow);
        }
        Ok(EnumDef {
            type_id,
            name,
            wire,
            entries,
        })
    }

    /// Nombre del valor, si está entre las entradas.
    pub fn name_sym_of(&self, value: i64) -> Option<u16> {
        self.entries
            .iter()
            .find(|(v, _)| *v == value)
            .map(|(_, s)| *s)
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

/// Registro de tipos conocidos, poblado con los `REC_TYPE_DEF` y
/// `REC_ENUM_DEF` recibidos. El espacio de `type_id` es uno solo (REC-53).
#[derive(Debug, Default)]
pub struct TypeRegistry {
    map: HashMap<u16, TypeDef>,
    enums: HashMap<u16, EnumDef>,
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

    pub fn insert_enum(&mut self, def: EnumDef) {
        self.enums.insert(def.type_id, def);
    }

    pub fn get_enum(&self, type_id: u16) -> Option<&EnumDef> {
        self.enums.get(&type_id)
    }
}
