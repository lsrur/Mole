//! Constantes de wire — spec §6/§7 (draft.10). Sin lógica: espejo de `mole_wire.h`.

/// Tamaño máximo de frame pre-COBS (PR-04).
pub const FRAME_MAX: usize = 4096;
/// Tamaño del header de frame (PR-02): ver_flags + seq + t_base_us + rec_count.
pub const FRAME_HEADER_LEN: usize = 1 + 2 + 8 + 2;
/// Tamaño del header de record (PR-07).
pub const RECORD_HEADER_LEN: usize = 4;
/// Magic de CTL_RESET (PR-18, SEC-02): "MOLE".
pub const RESET_MAGIC: u32 = 0x4D4F_4C45;
/// Profundidad máxima de structs anidados (REC-46).
pub const MAX_STRUCT_DEPTH: usize = 4;
/// `sym_id` nulo (CAT-06) y de overflow (CAT-05).
pub const SYM_NONE: u16 = 0;
pub const SYM_OVERFLOW: u16 = 0xFFFF;

// Tipos de record, uplink — §7.1.
pub const REC_SESSION: u8 = 0x01;
pub const REC_SYM_DEF: u8 = 0x02;
pub const REC_STATS: u8 = 0x03;
pub const REC_PONG: u8 = 0x04;
pub const REC_FMT_DEF: u8 = 0x05;
pub const REC_TYPE_DEF: u8 = 0x06;
pub const REC_ENUM_DEF: u8 = 0x07;
pub const REC_LOG: u8 = 0x10;
pub const REC_LOG_FMT: u8 = 0x11;
pub const REC_WATCH: u8 = 0x20;
pub const REC_WATCH_STR: u8 = 0x21;
pub const REC_BIND_DEF: u8 = 0x30;
pub const REC_BIND_VAL: u8 = 0x31;
pub const REC_DUMP: u8 = 0x40;
pub const REC_SCHEMA_DEF: u8 = 0x41;
pub const REC_SPAN_BEGIN: u8 = 0x50;
pub const REC_SPAN_END: u8 = 0x51;
pub const REC_SPAN_ABORT: u8 = 0x52;
pub const REC_COUNTER: u8 = 0x60;
pub const REC_STATE: u8 = 0x70;
pub const REC_STATUS: u8 = 0x71;
pub const REC_CHECK_FAIL: u8 = 0x80;
pub const REC_EVENT: u8 = 0x81;
pub const REC_CMD_DEF: u8 = 0x90;
pub const REC_BLOB_BEGIN: u8 = 0xA0;
pub const REC_BLOB_CHUNK: u8 = 0xA1;
pub const REC_PAUSED: u8 = 0xF0;
pub const REC_RESUMED: u8 = 0xF1;

// Tipos de record, downlink — PR-18 (draft.10).
pub const CTL_HELLO: u8 = 0xC0;
pub const CTL_CMD: u8 = 0xC1;
pub const CTL_BIND_SET: u8 = 0xC2;
pub const CTL_PAUSE: u8 = 0xC3;
pub const CTL_RESUME: u8 = 0xC4;
pub const CTL_STEP: u8 = 0xC5;
pub const CTL_RESET: u8 = 0xC6;
pub const CTL_SET_LEVEL: u8 = 0xC7;
pub const CTL_SET_POLICY: u8 = 0xC8;
pub const CTL_PING: u8 = 0xC9;

// Flags de frame — PR-02.
pub const FLAG_CATALOG: u8 = 1 << 0; // bit 4 del ver_flags, normalizado al nibble alto
pub const FLAG_DROPS: u8 = 1 << 1;
pub const FLAG_PAUSED: u8 = 1 << 2;

/// Enum de tipos de wire — REC-52. Un solo enum para args, watch, bind y
/// descriptores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    Bool,
    /// Cadena literal internada: 2 bytes de `sym_id`.
    Sym,
    /// Cadena de runtime: prefijo u8 + bytes (PR-20).
    Str,
    /// Puntero: 4 bytes de dirección.
    Ptr,
    /// Struct con descriptor: ver REC-44/REC-45.
    Struct,
}

impl WireType {
    pub fn from_u8(v: u8) -> Option<WireType> {
        Some(match v {
            0x01 => WireType::U8,
            0x02 => WireType::I8,
            0x03 => WireType::U16,
            0x04 => WireType::I16,
            0x05 => WireType::U32,
            0x06 => WireType::I32,
            0x07 => WireType::U64,
            0x08 => WireType::I64,
            0x09 => WireType::F32,
            0x0A => WireType::F64,
            0x0B => WireType::Bool,
            0x0C => WireType::Sym,
            0x0D => WireType::Str,
            0x0E => WireType::Ptr,
            0xF0 => WireType::Struct,
            _ => return None,
        })
    }

    pub fn to_u8(self) -> u8 {
        match self {
            WireType::U8 => 0x01,
            WireType::I8 => 0x02,
            WireType::U16 => 0x03,
            WireType::I16 => 0x04,
            WireType::U32 => 0x05,
            WireType::I32 => 0x06,
            WireType::U64 => 0x07,
            WireType::I64 => 0x08,
            WireType::F32 => 0x09,
            WireType::F64 => 0x0A,
            WireType::Bool => 0x0B,
            WireType::Sym => 0x0C,
            WireType::Str => 0x0D,
            WireType::Ptr => 0x0E,
            WireType::Struct => 0xF0,
        }
    }

    /// Tamaño fijo en el wire; `None` para `Str` y `Struct` (variables).
    pub fn fixed_size(self) -> Option<usize> {
        Some(match self {
            WireType::U8 | WireType::I8 | WireType::Bool => 1,
            WireType::U16 | WireType::I16 | WireType::Sym => 2,
            WireType::U32 | WireType::I32 | WireType::F32 | WireType::Ptr => 4,
            WireType::U64 | WireType::I64 | WireType::F64 => 8,
            WireType::Str | WireType::Struct => return None,
        })
    }

    /// ¿Es un tipo numérico escalar? (dimensiona `min[]/max[]/step[]`, REC-10/REC-29)
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            WireType::U8
                | WireType::I8
                | WireType::U16
                | WireType::I16
                | WireType::U32
                | WireType::I32
                | WireType::U64
                | WireType::I64
                | WireType::F32
                | WireType::F64
        )
    }

    /// Nombre canónico, el mismo que usan los vectores JSON.
    pub fn name(self) -> &'static str {
        match self {
            WireType::U8 => "u8",
            WireType::I8 => "i8",
            WireType::U16 => "u16",
            WireType::I16 => "i16",
            WireType::U32 => "u32",
            WireType::I32 => "i32",
            WireType::U64 => "u64",
            WireType::I64 => "i64",
            WireType::F32 => "f32",
            WireType::F64 => "f64",
            WireType::Bool => "bool",
            WireType::Sym => "sym",
            WireType::Str => "str",
            WireType::Ptr => "ptr",
            WireType::Struct => "struct",
        }
    }
}

/// Tipos de símbolo — CAT-03.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymKind {
    Watch = 1,
    Tag = 2,
    Task = 3,
    File = 4,
    Command = 5,
    Span = 6,
    Counter = 7,
    Machine = 8,
    State = 9,
    Bind = 10,
    Dump = 11,
    Schema = 12,
}

/// Bit 0 de `flags` por campo en `REC_TYPE_DEF` (draft.9): big-endian.
pub const FIELD_FLAG_BE: u8 = 0x01;
/// Bit 1 (draft.11, REC-54): campo arreglo, `count = size / tamaño(wire)`.
pub const FIELD_FLAG_ARRAY: u8 = 0x02;
/// Bit 2 (draft.11, REC-53): campo enum, `ref_type` = type_id del enum.
pub const FIELD_FLAG_ENUM: u8 = 0x04;
/// Tag de `arg_types` para un enum suelto (draft.11, REC-53): sigue el
/// `type_id` inline, como 0xF0 para structs.
pub const ARG_TAG_ENUM: u8 = 0xF1;
