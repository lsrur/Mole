//! Records — spec §7.1 (draft.10), uplink y downlink.
//!
//! El header (PR-07) es `{ type: u8, len: u8, dt_us: u16 }`. Un `type`
//! desconocido decodifica como [`Record::Unknown`], nunca falla (§6.7 del
//! plan). Los argumentos de `REC_LOG_FMT`/`REC_CHECK_FAIL` quedan crudos acá:
//! su decodificación tipada necesita el `REC_FMT_DEF` (ver `catalog`).

use crate::args::{decode_arg_types, encode_arg_types, decode_value, encode_value, ArgType, Value};
use crate::error::DecodeError;
use crate::rw::{put_str, put_u16, put_u32, Reader};
use crate::types::{TypeDef, TypeRegistry};
use crate::wire::{self, WireType};

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub epoch: u32,
    pub catalog_hash: u32,
    pub chip_model: u16,
    pub chip_rev: u16,
    pub idf_ver: Vec<u8>,
    pub app_name: Vec<u8>,
    pub app_build_time: Vec<u8>,
    pub app_elf_sha256: [u8; 8],
    pub cpu_freq_mhz: u16,
    pub free_heap: u32,
    pub mole_ver: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub enqueued: u32,
    pub dropped: u32,
    pub dropped_by_kind: [u16; 8],
    pub ring_high_water: u16,
    pub sym_overflow: u16,
    pub tx_bytes: u32,
    pub free_heap: u32,
    pub min_free_heap: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FmtDef {
    pub fmt_id: u16,
    pub file_sym: u16,
    pub line: u16,
    pub arg_types: Vec<ArgType>,
    pub fmt: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Session(Session),
    SymDef { sym_id: u16, kind: u8, parent: u16, name: Vec<u8> },
    Stats(Stats),
    Pong { nonce: u32 },
    FmtDef(FmtDef),
    TypeDef(TypeDef),
    Log { level: u8, task_id: u8, core: u8, tag_sym: u16, file_sym: u16, line: u16, msg: Vec<u8> },
    LogFmt { level: u8, task_id: u8, core: u8, tag_sym: u16, fmt_id: u16, args_raw: Vec<u8> },
    Watch { sym: u16, value: Value },
    WatchStr { sym: u16, s: Vec<u8> },
    BindDef { sym: u16, ty: WireType, flags: u8, min: Option<Value>, max: Option<Value>, step: Option<Value> },
    BindVal { sym: u16, value: Value },
    Dump { sym: u16, schema_sym: u16, flags: u8, addr: u32, data: Vec<u8> },
    SchemaDef { sym: u16, def: Vec<u8> },
    SpanBegin { span_id: u16, sym: u16, parent_span_id: u16, task_id: u8 },
    SpanEnd { span_id: u16 },
    SpanAbort { span_id: u16, reason: u8 },
    Counter { entries: Vec<(u16, u32)> },
    State { machine_sym: u16, state_sym: u16 },
    Status { sym: u16, level: u8 },
    CheckFail { fmt_id: u16, task_id: u8, core: u8, args_raw: Vec<u8> },
    Event { sym: u16, arg: u32 },
    CmdDef { cmd_id: u16, sym: u16, arg_type: u8, min: Option<Value>, max: Option<Value> },
    BlobBegin { blob_id: u16, sym: u16, total_len: u32, schema_sym: u16 },
    BlobChunk { blob_id: u16, offset: u32, data: Vec<u8> },
    Paused { task_id: u8, file_sym: u16, line: u16, reason: u8 },
    Resumed { task_id: u8, reason: u8 },
    CtlHello { proto_ver: u8, known_epoch: u32, known_catalog_hash: u32 },
    CtlCmd { cmd_id: u16, arg_type: u8, arg: Option<Value> },
    CtlBindSet { sym_id: u16, value: Value },
    CtlPause { scope: u8, task_id: u8 },
    CtlResume { scope: u8, task_id: u8 },
    CtlStep { task_id: u8 },
    CtlReset { magic: u32 },
    CtlSetLevel { sym_id: u16, level: u8 },
    CtlSetPolicy { kind: u8, policy: u8 },
    CtlPing { nonce: u32 },
    Unknown { rtype: u8, payload: Vec<u8> },
}

/// Valor auto-descripto `{ type: u8, value[] }` (watch/bind).
fn decode_typed_value(r: &mut Reader, reg: &TypeRegistry) -> Result<Value, DecodeError> {
    let raw = r.u8()?;
    let wt = WireType::from_u8(raw).ok_or(DecodeError::Truncated)?;
    let at = if wt == WireType::Struct {
        ArgType::Struct(r.u16()?)
    } else {
        ArgType::Scalar(wt)
    };
    decode_value(at, r, reg, 0)
}

fn encode_typed_value(out: &mut Vec<u8>, v: &Value, reg: &TypeRegistry) -> Result<(), DecodeError> {
    out.push(v.wire_type().to_u8());
    if let Value::Struct { type_id, .. } = v {
        put_u16(out, *type_id);
    }
    encode_value(v, out, reg, 0)
}

/// `min[]/max[]/step[]` — presentes solo para tipos numéricos (REC-10/REC-29).
fn decode_bound(r: &mut Reader, wt: WireType) -> Result<Option<Value>, DecodeError> {
    if !wt.is_numeric() {
        return Ok(None);
    }
    decode_value(ArgType::Scalar(wt), r, &TypeRegistry::new(), 0).map(Some)
}

impl Record {
    pub fn rtype(&self) -> u8 {
        use wire::*;
        match self {
            Record::Session(_) => REC_SESSION,
            Record::SymDef { .. } => REC_SYM_DEF,
            Record::Stats(_) => REC_STATS,
            Record::Pong { .. } => REC_PONG,
            Record::FmtDef(_) => REC_FMT_DEF,
            Record::TypeDef(_) => REC_TYPE_DEF,
            Record::Log { .. } => REC_LOG,
            Record::LogFmt { .. } => REC_LOG_FMT,
            Record::Watch { .. } => REC_WATCH,
            Record::WatchStr { .. } => REC_WATCH_STR,
            Record::BindDef { .. } => REC_BIND_DEF,
            Record::BindVal { .. } => REC_BIND_VAL,
            Record::Dump { .. } => REC_DUMP,
            Record::SchemaDef { .. } => REC_SCHEMA_DEF,
            Record::SpanBegin { .. } => REC_SPAN_BEGIN,
            Record::SpanEnd { .. } => REC_SPAN_END,
            Record::SpanAbort { .. } => REC_SPAN_ABORT,
            Record::Counter { .. } => REC_COUNTER,
            Record::State { .. } => REC_STATE,
            Record::Status { .. } => REC_STATUS,
            Record::CheckFail { .. } => REC_CHECK_FAIL,
            Record::Event { .. } => REC_EVENT,
            Record::CmdDef { .. } => REC_CMD_DEF,
            Record::BlobBegin { .. } => REC_BLOB_BEGIN,
            Record::BlobChunk { .. } => REC_BLOB_CHUNK,
            Record::Paused { .. } => REC_PAUSED,
            Record::Resumed { .. } => REC_RESUMED,
            Record::CtlHello { .. } => CTL_HELLO,
            Record::CtlCmd { .. } => CTL_CMD,
            Record::CtlBindSet { .. } => CTL_BIND_SET,
            Record::CtlPause { .. } => CTL_PAUSE,
            Record::CtlResume { .. } => CTL_RESUME,
            Record::CtlStep { .. } => CTL_STEP,
            Record::CtlReset { .. } => CTL_RESET,
            Record::CtlSetLevel { .. } => CTL_SET_LEVEL,
            Record::CtlSetPolicy { .. } => CTL_SET_POLICY,
            Record::CtlPing { .. } => CTL_PING,
            Record::Unknown { rtype, .. } => *rtype,
        }
    }

    /// Codifica solo el payload (sin header PR-07).
    pub fn encode_payload(&self, reg: &TypeRegistry) -> Result<Vec<u8>, DecodeError> {
        let mut o = Vec::new();
        match self {
            Record::Session(s) => {
                put_u32(&mut o, s.epoch);
                put_u32(&mut o, s.catalog_hash);
                put_u16(&mut o, s.chip_model);
                put_u16(&mut o, s.chip_rev);
                put_str(&mut o, &s.idf_ver)?;
                put_str(&mut o, &s.app_name)?;
                put_str(&mut o, &s.app_build_time)?;
                o.extend_from_slice(&s.app_elf_sha256);
                put_u16(&mut o, s.cpu_freq_mhz);
                put_u32(&mut o, s.free_heap);
                put_str(&mut o, &s.mole_ver)?;
            }
            Record::SymDef { sym_id, kind, parent, name } => {
                put_u16(&mut o, *sym_id);
                o.push(*kind);
                put_u16(&mut o, *parent);
                put_str(&mut o, name)?;
            }
            Record::Stats(s) => {
                put_u32(&mut o, s.enqueued);
                put_u32(&mut o, s.dropped);
                for d in s.dropped_by_kind {
                    put_u16(&mut o, d);
                }
                put_u16(&mut o, s.ring_high_water);
                put_u16(&mut o, s.sym_overflow);
                put_u32(&mut o, s.tx_bytes);
                put_u32(&mut o, s.free_heap);
                put_u32(&mut o, s.min_free_heap);
            }
            Record::Pong { nonce } => put_u32(&mut o, *nonce),
            Record::FmtDef(f) => {
                put_u16(&mut o, f.fmt_id);
                put_u16(&mut o, f.file_sym);
                put_u16(&mut o, f.line);
                if f.arg_types.len() > 255 {
                    return Err(DecodeError::LenOverflow);
                }
                o.push(f.arg_types.len() as u8);
                encode_arg_types(&mut o, &f.arg_types);
                put_str(&mut o, &f.fmt)?;
            }
            Record::TypeDef(t) => o = t.encode_payload()?,
            Record::Log { level, task_id, core, tag_sym, file_sym, line, msg } => {
                o.push(*level);
                o.push(*task_id);
                o.push(*core);
                put_u16(&mut o, *tag_sym);
                put_u16(&mut o, *file_sym);
                put_u16(&mut o, *line);
                put_str(&mut o, msg)?;
            }
            Record::LogFmt { level, task_id, core, tag_sym, fmt_id, args_raw } => {
                o.push(*level);
                o.push(*task_id);
                o.push(*core);
                put_u16(&mut o, *tag_sym);
                put_u16(&mut o, *fmt_id);
                o.extend_from_slice(args_raw);
            }
            Record::Watch { sym, value } => {
                put_u16(&mut o, *sym);
                encode_typed_value(&mut o, value, reg)?;
            }
            Record::WatchStr { sym, s } => {
                put_u16(&mut o, *sym);
                put_str(&mut o, s)?;
            }
            Record::BindDef { sym, ty, flags, min, max, step } => {
                put_u16(&mut o, *sym);
                o.push(ty.to_u8());
                o.push(*flags);
                for b in [min, max, step].into_iter().flatten() {
                    encode_value(b, &mut o, reg, 0)?;
                }
            }
            Record::BindVal { sym, value } => {
                put_u16(&mut o, *sym);
                encode_typed_value(&mut o, value, reg)?;
            }
            Record::Dump { sym, schema_sym, flags, addr, data } => {
                put_u16(&mut o, *sym);
                put_u16(&mut o, *schema_sym);
                o.push(*flags);
                put_u32(&mut o, *addr);
                o.extend_from_slice(data);
            }
            Record::SchemaDef { sym, def } => {
                put_u16(&mut o, *sym);
                put_str(&mut o, def)?;
            }
            Record::SpanBegin { span_id, sym, parent_span_id, task_id } => {
                put_u16(&mut o, *span_id);
                put_u16(&mut o, *sym);
                put_u16(&mut o, *parent_span_id);
                o.push(*task_id);
            }
            Record::SpanEnd { span_id } => put_u16(&mut o, *span_id),
            Record::SpanAbort { span_id, reason } => {
                put_u16(&mut o, *span_id);
                o.push(*reason);
            }
            Record::Counter { entries } => {
                if entries.len() > 255 {
                    return Err(DecodeError::LenOverflow);
                }
                o.push(entries.len() as u8);
                for (sym, delta) in entries {
                    put_u16(&mut o, *sym);
                    put_u32(&mut o, *delta);
                }
            }
            Record::State { machine_sym, state_sym } => {
                put_u16(&mut o, *machine_sym);
                put_u16(&mut o, *state_sym);
            }
            Record::Status { sym, level } => {
                put_u16(&mut o, *sym);
                o.push(*level);
            }
            Record::CheckFail { fmt_id, task_id, core, args_raw } => {
                put_u16(&mut o, *fmt_id);
                o.push(*task_id);
                o.push(*core);
                o.extend_from_slice(args_raw);
            }
            Record::Event { sym, arg } => {
                put_u16(&mut o, *sym);
                put_u32(&mut o, *arg);
            }
            Record::CmdDef { cmd_id, sym, arg_type, min, max } => {
                put_u16(&mut o, *cmd_id);
                put_u16(&mut o, *sym);
                o.push(*arg_type);
                for b in [min, max].into_iter().flatten() {
                    encode_value(b, &mut o, reg, 0)?;
                }
            }
            Record::BlobBegin { blob_id, sym, total_len, schema_sym } => {
                put_u16(&mut o, *blob_id);
                put_u16(&mut o, *sym);
                put_u32(&mut o, *total_len);
                put_u16(&mut o, *schema_sym);
            }
            Record::BlobChunk { blob_id, offset, data } => {
                put_u16(&mut o, *blob_id);
                put_u32(&mut o, *offset);
                o.extend_from_slice(data);
            }
            Record::Paused { task_id, file_sym, line, reason } => {
                o.push(*task_id);
                put_u16(&mut o, *file_sym);
                put_u16(&mut o, *line);
                o.push(*reason);
            }
            Record::Resumed { task_id, reason } => {
                o.push(*task_id);
                o.push(*reason);
            }
            Record::CtlHello { proto_ver, known_epoch, known_catalog_hash } => {
                o.push(*proto_ver);
                put_u32(&mut o, *known_epoch);
                put_u32(&mut o, *known_catalog_hash);
            }
            Record::CtlCmd { cmd_id, arg_type, arg } => {
                put_u16(&mut o, *cmd_id);
                o.push(*arg_type);
                if let Some(v) = arg {
                    encode_value(v, &mut o, reg, 0)?;
                }
            }
            Record::CtlBindSet { sym_id, value } => {
                put_u16(&mut o, *sym_id);
                encode_typed_value(&mut o, value, reg)?;
            }
            Record::CtlPause { scope, task_id } | Record::CtlResume { scope, task_id } => {
                o.push(*scope);
                o.push(*task_id);
            }
            Record::CtlStep { task_id } => o.push(*task_id),
            Record::CtlReset { magic } => put_u32(&mut o, *magic),
            Record::CtlSetLevel { sym_id, level } => {
                put_u16(&mut o, *sym_id);
                o.push(*level);
            }
            Record::CtlSetPolicy { kind, policy } => {
                o.push(*kind);
                o.push(*policy);
            }
            Record::CtlPing { nonce } => put_u32(&mut o, *nonce),
            Record::Unknown { payload, .. } => o = payload.clone(),
        }
        if o.len() > 255 {
            return Err(DecodeError::LenOverflow);
        }
        Ok(o)
    }

    /// Decodifica un payload según su tipo. Tipo desconocido → `Unknown`.
    /// El payload debe consumirse exacto (salvo campos trailing `data[]`).
    pub fn decode_payload(rtype: u8, payload: &[u8], reg: &TypeRegistry) -> Result<Record, DecodeError> {
        use wire::*;
        let mut r = Reader::new(payload);
        let rec = match rtype {
            REC_SESSION => {
                let epoch = r.u32()?;
                let catalog_hash = r.u32()?;
                let chip_model = r.u16()?;
                let chip_rev = r.u16()?;
                let idf_ver = r.str_pr20()?.to_vec();
                let app_name = r.str_pr20()?.to_vec();
                let app_build_time = r.str_pr20()?.to_vec();
                let sha = r.bytes(8)?;
                let mut app_elf_sha256 = [0u8; 8];
                app_elf_sha256.copy_from_slice(sha);
                Record::Session(Session {
                    epoch,
                    catalog_hash,
                    chip_model,
                    chip_rev,
                    idf_ver,
                    app_name,
                    app_build_time,
                    app_elf_sha256,
                    cpu_freq_mhz: r.u16()?,
                    free_heap: r.u32()?,
                    mole_ver: r.str_pr20()?.to_vec(),
                })
            }
            REC_SYM_DEF => Record::SymDef {
                sym_id: r.u16()?,
                kind: r.u8()?,
                parent: r.u16()?,
                name: r.str_pr20()?.to_vec(),
            },
            REC_STATS => {
                let enqueued = r.u32()?;
                let dropped = r.u32()?;
                let mut dropped_by_kind = [0u16; 8];
                for d in &mut dropped_by_kind {
                    *d = r.u16()?;
                }
                Record::Stats(Stats {
                    enqueued,
                    dropped,
                    dropped_by_kind,
                    ring_high_water: r.u16()?,
                    sym_overflow: r.u16()?,
                    tx_bytes: r.u32()?,
                    free_heap: r.u32()?,
                    min_free_heap: r.u32()?,
                })
            }
            REC_PONG => Record::Pong { nonce: r.u32()? },
            REC_FMT_DEF => {
                let fmt_id = r.u16()?;
                let file_sym = r.u16()?;
                let line = r.u16()?;
                let argc = r.u8()? as usize;
                let arg_types = decode_arg_types(&mut r, argc)?;
                let fmt = r.str_pr20()?.to_vec();
                Record::FmtDef(FmtDef { fmt_id, file_sym, line, arg_types, fmt })
            }
            // TypeDef parsea el payload completo por su cuenta; devolvemos
            // directo para no disparar el chequeo de sobrantes sobre un
            // Reader que quedó sin avanzar.
            REC_TYPE_DEF => return Ok(Record::TypeDef(TypeDef::decode_payload(payload)?)),
            REC_LOG => Record::Log {
                level: r.u8()?,
                task_id: r.u8()?,
                core: r.u8()?,
                tag_sym: r.u16()?,
                file_sym: r.u16()?,
                line: r.u16()?,
                msg: r.str_pr20()?.to_vec(),
            },
            REC_LOG_FMT => Record::LogFmt {
                level: r.u8()?,
                task_id: r.u8()?,
                core: r.u8()?,
                tag_sym: r.u16()?,
                fmt_id: r.u16()?,
                args_raw: r.rest().to_vec(),
            },
            REC_WATCH => Record::Watch {
                sym: r.u16()?,
                value: decode_typed_value(&mut r, reg)?,
            },
            REC_WATCH_STR => Record::WatchStr {
                sym: r.u16()?,
                s: r.str_pr20()?.to_vec(),
            },
            REC_BIND_DEF => {
                let sym = r.u16()?;
                let ty_raw = r.u8()?;
                let ty = WireType::from_u8(ty_raw).ok_or(DecodeError::Truncated)?;
                let flags = r.u8()?;
                Record::BindDef {
                    sym,
                    ty,
                    flags,
                    min: decode_bound(&mut r, ty)?,
                    max: decode_bound(&mut r, ty)?,
                    step: decode_bound(&mut r, ty)?,
                }
            }
            REC_BIND_VAL => Record::BindVal {
                sym: r.u16()?,
                value: decode_typed_value(&mut r, reg)?,
            },
            REC_DUMP => Record::Dump {
                sym: r.u16()?,
                schema_sym: r.u16()?,
                flags: r.u8()?,
                addr: r.u32()?,
                data: r.rest().to_vec(),
            },
            REC_SCHEMA_DEF => Record::SchemaDef {
                sym: r.u16()?,
                def: r.str_pr20()?.to_vec(),
            },
            REC_SPAN_BEGIN => Record::SpanBegin {
                span_id: r.u16()?,
                sym: r.u16()?,
                parent_span_id: r.u16()?,
                task_id: r.u8()?,
            },
            REC_SPAN_END => Record::SpanEnd { span_id: r.u16()? },
            REC_SPAN_ABORT => Record::SpanAbort {
                span_id: r.u16()?,
                reason: r.u8()?,
            },
            REC_COUNTER => {
                let n = r.u8()? as usize;
                let mut entries = Vec::with_capacity(n.min(64));
                for _ in 0..n {
                    entries.push((r.u16()?, r.u32()?));
                }
                Record::Counter { entries }
            }
            REC_STATE => Record::State {
                machine_sym: r.u16()?,
                state_sym: r.u16()?,
            },
            REC_STATUS => Record::Status {
                sym: r.u16()?,
                level: r.u8()?,
            },
            REC_CHECK_FAIL => Record::CheckFail {
                fmt_id: r.u16()?,
                task_id: r.u8()?,
                core: r.u8()?,
                args_raw: r.rest().to_vec(),
            },
            REC_EVENT => Record::Event {
                sym: r.u16()?,
                arg: r.u32()?,
            },
            REC_CMD_DEF => {
                let cmd_id = r.u16()?;
                let sym = r.u16()?;
                let arg_type = r.u8()?;
                let (min, max) = if arg_type == 0 {
                    (None, None)
                } else {
                    let ty = WireType::from_u8(arg_type).ok_or(DecodeError::Truncated)?;
                    (decode_bound(&mut r, ty)?, decode_bound(&mut r, ty)?)
                };
                Record::CmdDef { cmd_id, sym, arg_type, min, max }
            }
            REC_BLOB_BEGIN => Record::BlobBegin {
                blob_id: r.u16()?,
                sym: r.u16()?,
                total_len: r.u32()?,
                schema_sym: r.u16()?,
            },
            REC_BLOB_CHUNK => Record::BlobChunk {
                blob_id: r.u16()?,
                offset: r.u32()?,
                data: r.rest().to_vec(),
            },
            REC_PAUSED => Record::Paused {
                task_id: r.u8()?,
                file_sym: r.u16()?,
                line: r.u16()?,
                reason: r.u8()?,
            },
            REC_RESUMED => Record::Resumed {
                task_id: r.u8()?,
                reason: r.u8()?,
            },
            CTL_HELLO => Record::CtlHello {
                proto_ver: r.u8()?,
                known_epoch: r.u32()?,
                known_catalog_hash: r.u32()?,
            },
            CTL_CMD => {
                let cmd_id = r.u16()?;
                let arg_type = r.u8()?;
                let arg = if arg_type == 0 {
                    None
                } else {
                    let wt = WireType::from_u8(arg_type).ok_or(DecodeError::Truncated)?;
                    let at = if wt == WireType::Struct {
                        ArgType::Struct(r.u16()?)
                    } else {
                        ArgType::Scalar(wt)
                    };
                    Some(decode_value(at, &mut r, reg, 0)?)
                };
                Record::CtlCmd { cmd_id, arg_type, arg }
            }
            CTL_BIND_SET => Record::CtlBindSet {
                sym_id: r.u16()?,
                value: decode_typed_value(&mut r, reg)?,
            },
            CTL_PAUSE => Record::CtlPause {
                scope: r.u8()?,
                task_id: r.u8()?,
            },
            CTL_RESUME => Record::CtlResume {
                scope: r.u8()?,
                task_id: r.u8()?,
            },
            CTL_STEP => Record::CtlStep { task_id: r.u8()? },
            CTL_RESET => Record::CtlReset { magic: r.u32()? },
            CTL_SET_LEVEL => Record::CtlSetLevel {
                sym_id: r.u16()?,
                level: r.u8()?,
            },
            CTL_SET_POLICY => Record::CtlSetPolicy {
                kind: r.u8()?,
                policy: r.u8()?,
            },
            CTL_PING => Record::CtlPing { nonce: r.u32()? },
            _ => {
                return Ok(Record::Unknown {
                    rtype,
                    payload: payload.to_vec(),
                })
            }
        };
        if !r.is_empty() {
            return Err(DecodeError::LenOverflow);
        }
        Ok(rec)
    }

    /// Record completo con header PR-07.
    pub fn encode(&self, dt_us: u16, reg: &TypeRegistry) -> Result<Vec<u8>, DecodeError> {
        let payload = self.encode_payload(reg)?;
        let mut out = Vec::with_capacity(4 + payload.len());
        out.push(self.rtype());
        out.push(payload.len() as u8);
        out.extend_from_slice(&dt_us.to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::types::{FieldDef, TypeDef};
    use crate::wire::FIELD_FLAG_BE;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn ancla_rec_span_end() {
        let rec = Record::SpanEnd { span_id: 7 };
        let bytes = rec.encode(250, &TypeRegistry::new()).unwrap();
        assert_eq!(hex(&bytes), "5102fa000700");
    }

    #[test]
    fn ancla_rec_sym_def() {
        let rec = Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() };
        let bytes = rec.encode(0, &TypeRegistry::new()).unwrap();
        assert_eq!(hex(&bytes), "020900000100020000036e6574");
    }

    #[test]
    fn ancla_rec_fmt_def() {
        let rec = Record::FmtDef(FmtDef {
            fmt_id: 7,
            file_sym: 4,
            line: 42,
            arg_types: vec![ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)],
            fmt: b"crudo={} v={:.2}".to_vec(),
        });
        let bytes = rec.encode(0, &TypeRegistry::new()).unwrap();
        assert_eq!(
            hex(&bytes),
            "051a0000070004002a0002060910637275646f3d7b7d20763d7b3a2e327d"
        );
    }

    #[test]
    fn ancla_rec_log_fmt() {
        let rec = Record::LogFmt {
            level: 2,
            task_id: 3,
            core: 1,
            tag_sym: 1,
            fmt_id: 7,
            args_raw: vec![0x00, 0x04, 0x00, 0x00, 0x7b, 0x14, 0x1e, 0x40],
        };
        let bytes = rec.encode(1500, &TypeRegistry::new()).unwrap();
        assert_eq!(hex(&bytes), "110fdc0502030101000700000400007b141e40");
    }

    #[test]
    fn ancla_rec_watch() {
        let rec = Record::Watch { sym: 5, value: Value::F32(1.5) };
        let bytes = rec.encode(10, &TypeRegistry::new()).unwrap();
        assert_eq!(hex(&bytes), "20070a000500090000c03f");
    }

    #[test]
    fn ancla_rec_type_def() {
        let rec = Record::TypeDef(TypeDef {
            type_id: 1,
            name: b"Sens".to_vec(),
            fields: vec![
                FieldDef { name_sym: 2, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 },
                FieldDef { name_sym: 3, wire: WireType::U16, flags: FIELD_FLAG_BE, offset: 2, size: 2, ref_type: 0 },
            ],
        });
        let bytes = rec.encode(0, &TypeRegistry::new()).unwrap();
        assert_eq!(
            hex(&bytes),
            "061c000001000453656e73020200010000000100000003000301020002000000"
        );
    }

    #[test]
    fn roundtrip_todos_los_tipos() {
        let reg = TypeRegistry::new();
        let recs = vec![
            Record::Session(Session {
                epoch: 7,
                catalog_hash: 0xDEADBEEF,
                chip_model: 9,
                chip_rev: 301,
                idf_ver: b"v5.3".to_vec(),
                app_name: b"bench".to_vec(),
                app_build_time: b"2026-08-18".to_vec(),
                app_elf_sha256: [1, 2, 3, 4, 5, 6, 7, 8],
                cpu_freq_mhz: 240,
                free_heap: 200_000,
                mole_ver: b"2.0.0".to_vec(),
            }),
            Record::Stats(Stats {
                enqueued: 1000,
                dropped: 4,
                dropped_by_kind: [0, 1, 0, 0, 3, 0, 0, 0],
                ring_high_water: 512,
                sym_overflow: 0,
                tx_bytes: 123_456,
                free_heap: 180_000,
                min_free_heap: 150_000,
            }),
            Record::Pong { nonce: 0xCAFE_F00D },
            Record::Log {
                level: 4,
                task_id: 1,
                core: 0,
                tag_sym: 3,
                file_sym: 4,
                line: 99,
                msg: b"runtime armado".to_vec(),
            },
            Record::WatchStr { sym: 8, s: b"JOINED".to_vec() },
            Record::BindDef {
                sym: 9,
                ty: WireType::F32,
                flags: 0,
                min: Some(Value::F32(0.0)),
                max: Some(Value::F32(5.0)),
                step: Some(Value::F32(0.01)),
            },
            Record::BindDef {
                sym: 10,
                ty: WireType::Bool,
                flags: 0,
                min: None,
                max: None,
                step: None,
            },
            Record::BindVal { sym: 9, value: Value::F32(2.5) },
            Record::Dump {
                sym: 11,
                schema_sym: 0,
                flags: 0,
                addr: 0x3B,
                data: vec![0xAA; 14],
            },
            Record::SchemaDef { sym: 12, def: b"u8 ver; u16 src;".to_vec() },
            Record::SpanBegin { span_id: 100, sym: 6, parent_span_id: 0, task_id: 2 },
            Record::SpanAbort { span_id: 100, reason: 1 },
            Record::Counter { entries: vec![(13, 100_000), (14, 7)] },
            Record::State { machine_sym: 15, state_sym: 16 },
            Record::Status { sym: 17, level: 2 },
            Record::CheckFail { fmt_id: 3, task_id: 1, core: 1, args_raw: vec![0x01, 0x02] },
            Record::Event { sym: 18, arg: 42 },
            Record::CmdDef { cmd_id: 1, sym: 19, arg_type: 0, min: None, max: None },
            Record::CmdDef {
                cmd_id: 2,
                sym: 20,
                arg_type: WireType::I32.to_u8(),
                min: Some(Value::I32(0)),
                max: Some(Value::I32(15)),
            },
            Record::BlobBegin { blob_id: 1, sym: 21, total_len: 100_000, schema_sym: 0 },
            Record::BlobChunk { blob_id: 1, offset: 249, data: vec![0x55; 249] },
            Record::Paused { task_id: 2, file_sym: 4, line: 120, reason: 1 },
            Record::Resumed { task_id: 2, reason: 3 },
            Record::CtlHello { proto_ver: 2, known_epoch: 0, known_catalog_hash: 0 },
            Record::CtlCmd { cmd_id: 2, arg_type: WireType::I32.to_u8(), arg: Some(Value::I32(11)) },
            Record::CtlCmd { cmd_id: 1, arg_type: 0, arg: None },
            Record::CtlBindSet { sym_id: 9, value: Value::F32(3.3) },
            Record::CtlPause { scope: 1, task_id: 0 },
            Record::CtlResume { scope: 0, task_id: 2 },
            Record::CtlStep { task_id: 2 },
            Record::CtlReset { magic: crate::wire::RESET_MAGIC },
            Record::CtlSetLevel { sym_id: 3, level: 1 },
            Record::CtlSetPolicy { kind: 1, policy: 3 },
            Record::CtlPing { nonce: 1 },
        ];
        for rec in recs {
            let payload = rec.encode_payload(&reg).unwrap();
            let back = Record::decode_payload(rec.rtype(), &payload, &reg).unwrap();
            assert_eq!(back, rec, "roundtrip fallo para {:?}", rec.rtype());
        }
    }

    #[test]
    fn tipo_desconocido_decodifica_como_unknown() {
        let reg = TypeRegistry::new();
        let rec = Record::decode_payload(0xEE, &[1, 2, 3], &reg).unwrap();
        assert_eq!(rec, Record::Unknown { rtype: 0xEE, payload: vec![1, 2, 3] });
    }

    #[test]
    fn payload_con_sobra_es_len_overflow() {
        let reg = TypeRegistry::new();
        // REC_SPAN_END con un byte de más
        let res = Record::decode_payload(crate::wire::REC_SPAN_END, &[7, 0, 9], &reg);
        assert_eq!(res, Err(DecodeError::LenOverflow));
    }
}
