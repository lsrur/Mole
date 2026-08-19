//! Valores y empaquetado de argumentos — §7.2.3, REC-37/REC-38, REC-44/REC-45.
//!
//! Los argumentos de `REC_LOG_FMT` viajan sin etiquetas de tipo (los tipos
//! ya viajaron en el `REC_FMT_DEF`). El modo valores de structs es secuencial
//! y sin padding (Q-3): el `offset` del descriptor NO participa acá.

use crate::error::DecodeError;
use crate::rw::{put_str, Reader};
use crate::types::TypeRegistry;
use crate::wire::{WireType, MAX_STRUCT_DEPTH};

/// Un valor decodificado. Los floats conservan el patrón de bits exacto
/// (NaN con payload incluido); las cadenas conservan los bytes crudos y el
/// UTF-8 inválido se resuelve con `from_utf8_lossy` recién al renderizar.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    /// Cadena literal internada: viaja el `sym_id` (REC-38).
    Sym(u16),
    /// Cadena de runtime: bytes crudos, prefijo PR-20 en el wire.
    Str(Vec<u8>),
    Ptr(u32),
    Struct { type_id: u16, fields: Vec<Value> },
}

impl Value {
    pub fn wire_type(&self) -> WireType {
        match self {
            Value::U8(_) => WireType::U8,
            Value::I8(_) => WireType::I8,
            Value::U16(_) => WireType::U16,
            Value::I16(_) => WireType::I16,
            Value::U32(_) => WireType::U32,
            Value::I32(_) => WireType::I32,
            Value::U64(_) => WireType::U64,
            Value::I64(_) => WireType::I64,
            Value::F32(_) => WireType::F32,
            Value::F64(_) => WireType::F64,
            Value::Bool(_) => WireType::Bool,
            Value::Sym(_) => WireType::Sym,
            Value::Str(_) => WireType::Str,
            Value::Ptr(_) => WireType::Ptr,
            Value::Struct { .. } => WireType::Struct,
        }
    }
}

/// Tipo de argumento tal como viaja en `arg_types` de `REC_FMT_DEF`:
/// el tag `0xF0` va seguido inline de los 2 bytes del `type_id` (REC-44).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    Scalar(WireType),
    Struct(u16),
}

/// Decodifica la lista `arg_types` de un `REC_FMT_DEF`.
pub fn decode_arg_types(r: &mut Reader, argc: usize) -> Result<Vec<ArgType>, DecodeError> {
    let mut out = Vec::with_capacity(argc.min(32));
    for _ in 0..argc {
        let tag = r.u8()?;
        let wt = WireType::from_u8(tag).ok_or(DecodeError::Truncated)?;
        if wt == WireType::Struct {
            out.push(ArgType::Struct(r.u16()?));
        } else {
            out.push(ArgType::Scalar(wt));
        }
    }
    Ok(out)
}

/// Codifica la lista `arg_types` (REC-44).
pub fn encode_arg_types(out: &mut Vec<u8>, types: &[ArgType]) {
    for t in types {
        match t {
            ArgType::Scalar(wt) => out.push(wt.to_u8()),
            ArgType::Struct(type_id) => {
                out.push(WireType::Struct.to_u8());
                out.extend_from_slice(&type_id.to_le_bytes());
            }
        }
    }
}

/// Decodifica un valor escalar (no struct) de tipo conocido.
fn decode_scalar(wt: WireType, r: &mut Reader, big_endian: bool) -> Result<Value, DecodeError> {
    // Los campos big-endian viajan tal como están en memoria (el MCU nunca
    // invierte); el host invierte acá, al decodificar (Q-2 / REC-45).
    macro_rules! int {
        ($n:expr, $ty:ty, $variant:ident) => {{
            let b = r.bytes($n)?;
            let mut a = [0u8; $n];
            a.copy_from_slice(b);
            let v = if big_endian {
                <$ty>::from_be_bytes(a)
            } else {
                <$ty>::from_le_bytes(a)
            };
            Value::$variant(v)
        }};
    }
    Ok(match wt {
        WireType::U8 => Value::U8(r.u8()?),
        WireType::I8 => Value::I8(r.u8()? as i8),
        WireType::U16 => int!(2, u16, U16),
        WireType::I16 => int!(2, i16, I16),
        WireType::U32 => int!(4, u32, U32),
        WireType::I32 => int!(4, i32, I32),
        WireType::U64 => int!(8, u64, U64),
        WireType::I64 => int!(8, i64, I64),
        WireType::F32 => {
            let b = r.bytes(4)?;
            let mut a = [0u8; 4];
            a.copy_from_slice(b);
            let bits = if big_endian {
                u32::from_be_bytes(a)
            } else {
                u32::from_le_bytes(a)
            };
            Value::F32(f32::from_bits(bits))
        }
        WireType::F64 => {
            let b = r.bytes(8)?;
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            let bits = if big_endian {
                u64::from_be_bytes(a)
            } else {
                u64::from_le_bytes(a)
            };
            Value::F64(f64::from_bits(bits))
        }
        WireType::Bool => Value::Bool(r.u8()? != 0),
        WireType::Sym => int!(2, u16, U16).into_sym(),
        WireType::Ptr => int!(4, u32, U32).into_ptr(),
        WireType::Str => Value::Str(r.str_pr20()?.to_vec()),
        WireType::Struct => return Err(DecodeError::Truncated), // via decode_value
    })
}

impl Value {
    fn into_sym(self) -> Value {
        match self {
            Value::U16(v) => Value::Sym(v),
            other => other,
        }
    }
    fn into_ptr(self) -> Value {
        match self {
            Value::U32(v) => Value::Ptr(v),
            other => other,
        }
    }
}

/// Decodifica un valor según su `ArgType`, resolviendo structs por el
/// registro. Modo valores: campos en orden, sin padding, profundidad ≤ 4.
pub fn decode_value(
    at: ArgType,
    r: &mut Reader,
    reg: &TypeRegistry,
    depth: usize,
) -> Result<Value, DecodeError> {
    match at {
        ArgType::Scalar(wt) => decode_scalar(wt, r, false),
        ArgType::Struct(type_id) => {
            if depth >= MAX_STRUCT_DEPTH {
                return Err(DecodeError::DepthExceeded);
            }
            let def = reg.get(type_id).ok_or(DecodeError::UnknownFmt)?;
            let mut fields = Vec::with_capacity(def.fields.len());
            for f in &def.fields {
                let v = if f.wire == WireType::Struct {
                    decode_value(ArgType::Struct(f.ref_type), r, reg, depth + 1)?
                } else {
                    decode_scalar(f.wire, r, f.is_big_endian())?
                };
                fields.push(v);
            }
            Ok(Value::Struct { type_id, fields })
        }
    }
}

/// Codifica un valor escalar. `big_endian` reproduce el layout de memoria de
/// un campo BE (solo dentro de structs).
fn encode_scalar(v: &Value, out: &mut Vec<u8>, big_endian: bool) -> Result<(), DecodeError> {
    macro_rules! int {
        ($v:expr) => {{
            if big_endian {
                out.extend_from_slice(&$v.to_be_bytes());
            } else {
                out.extend_from_slice(&$v.to_le_bytes());
            }
        }};
    }
    match v {
        Value::U8(v) => out.push(*v),
        Value::I8(v) => out.push(*v as u8),
        Value::U16(v) => int!(v),
        Value::I16(v) => int!(v),
        Value::U32(v) => int!(v),
        Value::I32(v) => int!(v),
        Value::U64(v) => int!(v),
        Value::I64(v) => int!(v),
        Value::F32(v) => int!(v.to_bits()),
        Value::F64(v) => int!(v.to_bits()),
        Value::Bool(v) => out.push(*v as u8),
        Value::Sym(v) => int!(v),
        Value::Ptr(v) => int!(v),
        Value::Str(s) => put_str(out, s)?,
        Value::Struct { .. } => return Err(DecodeError::Truncated), // via encode_value
    }
    Ok(())
}

/// Codifica un valor, resolviendo structs por el registro (modo valores).
pub fn encode_value(
    v: &Value,
    out: &mut Vec<u8>,
    reg: &TypeRegistry,
    depth: usize,
) -> Result<(), DecodeError> {
    match v {
        Value::Struct { type_id, fields } => {
            if depth >= MAX_STRUCT_DEPTH {
                return Err(DecodeError::DepthExceeded);
            }
            let def = reg.get(*type_id).ok_or(DecodeError::UnknownFmt)?;
            if def.fields.len() != fields.len() {
                return Err(DecodeError::ArgCountMismatch);
            }
            for (f, v) in def.fields.iter().zip(fields) {
                if f.wire == WireType::Struct {
                    encode_value(v, out, reg, depth + 1)?;
                } else {
                    encode_scalar(v, out, f.is_big_endian())?;
                }
            }
            Ok(())
        }
        scalar => encode_scalar(scalar, out, false),
    }
}

/// Decodifica los argumentos de un `REC_LOG_FMT`/`REC_CHECK_FAIL` contra los
/// `arg_types` de su definición. El buffer debe consumirse exacto: sobrar o
/// faltar bytes es `arg_count_mismatch` (§6.7 del plan).
pub fn decode_args(
    arg_types: &[ArgType],
    raw: &[u8],
    reg: &TypeRegistry,
) -> Result<Vec<Value>, DecodeError> {
    let mut r = Reader::new(raw);
    let mut out = Vec::with_capacity(arg_types.len());
    for &at in arg_types {
        match decode_value(at, &mut r, reg, 0) {
            Ok(v) => out.push(v),
            Err(DecodeError::Truncated) => return Err(DecodeError::ArgCountMismatch),
            Err(e) => return Err(e),
        }
    }
    if !r.is_empty() {
        return Err(DecodeError::ArgCountMismatch);
    }
    Ok(out)
}

/// Codifica una lista de argumentos en orden, sin etiquetas (REC-35).
pub fn encode_args(
    args: &[Value],
    reg: &TypeRegistry,
) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    for v in args {
        encode_value(v, &mut out, reg, 0)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::types::{FieldDef, TypeDef};
    use crate::wire::FIELD_FLAG_BE;

    fn sens_registry() -> TypeRegistry {
        // struct Sens { uint8_t a; uint16_t b /* BE */; } — ancla rec-type-def
        let mut reg = TypeRegistry::new();
        reg.insert(TypeDef {
            type_id: 1,
            name: b"Sens".to_vec(),
            fields: vec![
                FieldDef { name_sym: 2, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 },
                FieldDef { name_sym: 3, wire: WireType::U16, flags: FIELD_FLAG_BE, offset: 2, size: 2, ref_type: 0 },
            ],
        });
        reg
    }

    #[test]
    fn ancla_args_struct_values() {
        // Memoria: a=0x11, pad, b=0x2233 BE. Wire modo valores: 11 22 33.
        let reg = sens_registry();
        let vals = decode_args(&[ArgType::Struct(1)], &[0x11, 0x22, 0x33], &reg).unwrap();
        assert_eq!(
            vals,
            vec![Value::Struct {
                type_id: 1,
                fields: vec![Value::U8(0x11), Value::U16(0x2233)]
            }]
        );
        // ida y vuelta: el encode reproduce el layout BE del wire
        assert_eq!(encode_args(&vals, &reg).unwrap(), vec![0x11, 0x22, 0x33]);
    }

    #[test]
    fn escalares_ancla_log_fmt() {
        // rec-log-fmt: i32 1024 + f32 2.47 → 00040000 7b141e40
        let reg = TypeRegistry::new();
        let types = [ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)];
        let raw = [0x00, 0x04, 0x00, 0x00, 0x7b, 0x14, 0x1e, 0x40];
        let vals = decode_args(&types, &raw, &reg).unwrap();
        assert_eq!(vals[0], Value::I32(1024));
        match vals[1] {
            Value::F32(f) => assert_eq!(f.to_bits(), 0x401E_147B),
            _ => panic!("tipo inesperado"),
        }
        assert_eq!(encode_args(&vals, &reg).unwrap(), raw);
    }

    #[test]
    fn nan_conserva_bits() {
        let reg = TypeRegistry::new();
        let raw = 0x7FC0_0001u32.to_le_bytes(); // NaN con payload
        let vals = decode_args(&[ArgType::Scalar(WireType::F32)], &raw, &reg).unwrap();
        assert_eq!(encode_args(&vals, &reg).unwrap(), raw);
    }

    #[test]
    fn cadenas_runtime_y_utf8_invalido() {
        let reg = TypeRegistry::new();
        let raw = [0x03, 0xFF, 0x61, 0x62]; // len 3, UTF-8 inválido
        let vals = decode_args(&[ArgType::Scalar(WireType::Str)], &raw, &reg).unwrap();
        assert_eq!(vals[0], Value::Str(vec![0xFF, 0x61, 0x62]));
        assert_eq!(encode_args(&vals, &reg).unwrap(), raw);
    }

    #[test]
    fn arg_count_mismatch() {
        let reg = TypeRegistry::new();
        // faltan bytes
        assert_eq!(
            decode_args(&[ArgType::Scalar(WireType::I32)], &[0x01], &reg),
            Err(DecodeError::ArgCountMismatch)
        );
        // sobran bytes
        assert_eq!(
            decode_args(&[ArgType::Scalar(WireType::U8)], &[0x01, 0x02], &reg),
            Err(DecodeError::ArgCountMismatch)
        );
    }

    #[test]
    fn profundidad_maxima() {
        // cadena de structs anidados: 5 niveles → DepthExceeded
        let mut reg = TypeRegistry::new();
        for id in 1..=5u16 {
            reg.insert(TypeDef {
                type_id: id,
                name: vec![b'T'],
                fields: vec![if id < 5 {
                    FieldDef { name_sym: 0, wire: WireType::Struct, flags: 0, offset: 0, size: 0, ref_type: id + 1 }
                } else {
                    FieldDef { name_sym: 0, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 }
                }],
            });
        }
        // profundidad 4 (structs 2..5 desde el 1): justo en el límite → falla al 5º nivel
        let res = decode_args(&[ArgType::Struct(1)], &[0x42], &reg);
        assert_eq!(res, Err(DecodeError::DepthExceeded));
        // una cadena de 4 structs (2..5) sí entra
        let res = decode_args(&[ArgType::Struct(2)], &[0x42], &reg);
        assert!(res.is_ok());
    }
}
