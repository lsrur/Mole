//! Genera y valida `protocol/codec_vectors.json` (T-12, plan §4/§6).
//!
//! El archivo se commitea; CI lo **valida**, no lo regenera (V-7). Los 12
//! ancla se generan acá con el codec de Rust y se verifican por el tercer
//! camino con `anchors.py --check` (V-2).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mole_codec::args::{decode_arg_types, decode_args, encode_args, ArgType, Value};
use mole_codec::catalog::Catalog;
use mole_codec::error::DecodeError;
use mole_codec::frame::{decode_wire, encode_frame};
use mole_codec::record::{FmtDef, Record, Session, Stats};
use mole_codec::types::{EnumDef, FieldDef, TypeDef, TypeRegistry};
use mole_codec::wire::{WireType, FIELD_FLAG_ARRAY, FIELD_FLAG_BE, FIELD_FLAG_ENUM};
use mole_codec::{cobs, crc32::crc32};
use serde_json::{json, Map, Value as J};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn cobs_vec(id: &str, desc: &str, decoded: &[u8], anchor: bool) -> J {
    let mut v = json!({
        "id": id, "layer": "cobs", "desc": desc,
        "decoded_hex": hex(decoded), "encoded_hex": hex(&cobs::encode(decoded)),
    });
    if anchor {
        v["anchor"] = json!(true);
    }
    v
}

fn crc_vec(id: &str, desc: &str, data: &[u8], anchor: bool) -> J {
    let mut v = json!({
        "id": id, "layer": "crc32", "desc": desc,
        "data_hex": hex(data), "crc_hex": format!("{:08x}", crc32(data)),
    });
    if anchor {
        v["anchor"] = json!(true);
    }
    v
}

fn rec_vec(id: &str, desc: &str, dt_us: u16, rec: &Record, rec_json: J, anchor: bool) -> J {
    let reg = TypeRegistry::new();
    let bytes = rec.encode(dt_us, &reg).expect("record de vector no codifica");
    let mut record_obj = match rec_json {
        J::Object(m) => m,
        _ => Map::new(),
    };
    record_obj.insert("dt_us".into(), json!(dt_us));
    let mut v = json!({
        "id": id, "layer": "record", "desc": desc,
        "record": record_obj, "bytes_hex": hex(&bytes),
    });
    if anchor {
        v["anchor"] = json!(true);
    }
    v
}

fn args_vec(
    id: &str,
    desc: &str,
    arg_types: &[ArgType],
    args: &[Value],
    args_json: J,
    reg: &TypeRegistry,
    requires: &[&str],
    anchor: bool,
) -> J {
    let mut th = Vec::new();
    mole_codec::args::encode_arg_types(&mut th, arg_types);
    let bytes = encode_args(args, reg).expect("args de vector no codifican");
    let mut v = json!({
        "id": id, "layer": "args", "desc": desc,
        "arg_types_hex": hex(&th), "args": args_json, "bytes_hex": hex(&bytes),
    });
    if !requires.is_empty() {
        v["requires"] = json!(requires);
    }
    if anchor {
        v["anchor"] = json!(true);
    }
    v
}

fn frame_vec(
    id: &str,
    desc: &str,
    seq: u16,
    t_base_us: u64,
    flags_nibble: u8,
    rec_refs: &[(&str, Vec<u8>)],
) -> J {
    let encoded: Vec<Vec<u8>> = rec_refs.iter().map(|(_, b)| b.clone()).collect();
    let (pre, wire) = encode_frame(seq, t_base_us, flags_nibble, &encoded).unwrap();
    let mut names = Vec::new();
    if flags_nibble & 0x1 != 0 {
        names.push("CATALOG");
    }
    if flags_nibble & 0x2 != 0 {
        names.push("DROPS");
    }
    if flags_nibble & 0x4 != 0 {
        names.push("PAUSED");
    }
    json!({
        "id": id, "layer": "frame", "desc": desc,
        "frame": {
            "ver": 2, "flags": names, "seq": seq, "t_base_us": t_base_us,
            "records": rec_refs.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
        },
        "pre_cobs_hex": hex(&pre), "wire_hex": hex(&wire),
    })
}

/// Los TYPE_DEF que usan los vectores de structs.
fn typedefs() -> Vec<(String, TypeDef)> {
    let mut out = Vec::new();
    out.push((
        "rec-type-def".into(),
        TypeDef {
            type_id: 1,
            name: b"Sens".to_vec(),
            fields: vec![
                FieldDef { name_sym: 2, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 },
                FieldDef { name_sym: 3, wire: WireType::U16, flags: FIELD_FLAG_BE, offset: 2, size: 2, ref_type: 0 },
            ],
        },
    ));
    out.push((
        "rec-type-def-padding-final".into(),
        TypeDef {
            type_id: 2,
            name: b"PadEnd".to_vec(),
            fields: vec![
                FieldDef { name_sym: 4, wire: WireType::U16, flags: 0, offset: 0, size: 2, ref_type: 0 },
                FieldDef { name_sym: 5, wire: WireType::U8, flags: 0, offset: 2, size: 1, ref_type: 0 },
            ],
        },
    ));
    out.push((
        "rec-type-def-interno".into(),
        TypeDef {
            type_id: 3,
            name: b"Inner".to_vec(),
            fields: vec![FieldDef { name_sym: 6, wire: WireType::I16, flags: 0, offset: 0, size: 2, ref_type: 0 }],
        },
    ));
    out.push((
        "rec-type-def-anidado".into(),
        TypeDef {
            type_id: 4,
            name: b"Outer".to_vec(),
            fields: vec![
                FieldDef { name_sym: 7, wire: WireType::Struct, flags: 0, offset: 0, size: 2, ref_type: 3 },
                FieldDef { name_sym: 8, wire: WireType::F32, flags: 0, offset: 4, size: 4, ref_type: 0 },
            ],
        },
    ));
    out.push((
        "rec-type-def-con-str".into(),
        TypeDef {
            type_id: 5,
            name: b"Msg".to_vec(),
            fields: vec![
                FieldDef { name_sym: 9, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 },
                FieldDef { name_sym: 10, wire: WireType::Str, flags: 0, offset: 0, size: 0, ref_type: 0 },
            ],
        },
    ));
    // draft.11: struct con campo arreglo y campo enum (REC-53/REC-54)
    out.push((
        "rec-type-def-imu".into(),
        TypeDef {
            type_id: 6,
            name: b"Imu".to_vec(),
            fields: vec![
                FieldDef { name_sym: 43, wire: WireType::F32, flags: FIELD_FLAG_ARRAY, offset: 0, size: 12, ref_type: 0 },
                FieldDef { name_sym: 44, wire: WireType::U8, flags: FIELD_FLAG_ENUM, offset: 12, size: 1, ref_type: 7 },
            ],
        },
    ));
    // cadena de profundidad: D1→D2→D3→D4 (válida) y E1→..→E5 (excede)
    for (i, id) in (11u16..=14).enumerate() {
        let last = id == 14;
        out.push((
            format!("rec-type-def-prof-{}", i + 1),
            TypeDef {
                type_id: id,
                name: format!("D{}", i + 1).into_bytes(),
                fields: vec![if last {
                    FieldDef { name_sym: 0, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 }
                } else {
                    FieldDef { name_sym: 0, wire: WireType::Struct, flags: 0, offset: 0, size: 1, ref_type: id + 1 }
                }],
            },
        ));
    }
    for (i, id) in (21u16..=25).enumerate() {
        let last = id == 25;
        out.push((
            format!("rec-type-def-exceso-{}", i + 1),
            TypeDef {
                type_id: id,
                name: format!("E{}", i + 1).into_bytes(),
                fields: vec![if last {
                    FieldDef { name_sym: 0, wire: WireType::U8, flags: 0, offset: 0, size: 1, ref_type: 0 }
                } else {
                    FieldDef { name_sym: 0, wire: WireType::Struct, flags: 0, offset: 0, size: 1, ref_type: id + 1 }
                }],
            },
        ));
    }
    out
}

fn typedef_json(t: &TypeDef) -> J {
    json!({
        "type": "REC_TYPE_DEF",
        "type_id": t.type_id,
        "name": String::from_utf8_lossy(&t.name),
        "nfields": t.fields.len(),
        "fields": t.fields.iter().map(|f| json!({
            "name_sym": f.name_sym, "wire": f.wire.name(), "flags": f.flags,
            "offset": f.offset, "size": f.size, "ref_type": f.ref_type,
        })).collect::<Vec<_>>(),
    })
}

/// El enum descripto que usan los vectores draft.11 (REC-53).
fn mode_enum() -> EnumDef {
    EnumDef {
        type_id: 7,
        name: b"Mode".to_vec(),
        wire: WireType::U8,
        entries: vec![(0, 40), (1, 41), (2, 42)], // RAW, AVG, MEDIAN
    }
}

pub fn build() -> J {
    let reg_all = {
        let mut r = TypeRegistry::new();
        for (_, t) in typedefs() {
            r.insert(t);
        }
        r.insert_enum(mode_enum());
        r
    };
    let mut v: Vec<J> = Vec::new();

    // ---------------- cobs (§6.1) ----------------
    v.push(cobs_vec("cobs-empty", "payload vacio: un solo codigo 0x01", &[], true));
    v.push(cobs_vec("cobs-one-byte", "un byte no cero", &[0x42], false));
    v.push(cobs_vec("cobs-no-zeros", "sin ceros", &[1, 2, 3, 4, 5], false));
    v.push(cobs_vec("cobs-zero-start", "cero al inicio", &[0, 0x11, 0x22], false));
    v.push(cobs_vec("cobs-zero-end", "cero al final", &[0x11, 0x22, 0], false));
    v.push(cobs_vec("cobs-zeros-consecutivos", "ceros consecutivos", &[0x11, 0, 0, 0x22], false));
    v.push(cobs_vec("cobs-zero-mid", "cero intercalado: bloques [11] y [22 33]", &[0x11, 0, 0x22, 0x33], true));
    v.push(cobs_vec("cobs-254", "bloque lleno de 254 no-ceros: FF + grupo final vacio (convencion fijada)", &[0x41; 254], true));
    v.push(cobs_vec("cobs-255", "255 no-ceros: bloque lleno + grupo de 1", &[0x41; 255], false));
    let frame_max: Vec<u8> = (0..4096u32).map(|i| (i % 256) as u8).collect();
    v.push(cobs_vec("cobs-frame-max", "payload de MOLE_FRAME_MAX (4096) con ceros periodicos", &frame_max, false));

    // ---------------- crc32 (§6.2) ----------------
    v.push(crc_vec("crc32-empty", "buffer vacio", &[], false));
    v.push(crc_vec("crc32-one-byte", "un byte 0x00", &[0], false));
    v.push(crc_vec("crc32-canonical", "check universal de CRC-32/ISO-HDLC; fija Q-1", b"123456789", true));
    v.push(crc_vec("crc32-plan-example", "ejemplo del plan §4", &[1, 2, 3, 4], false));
    let four_k: Vec<u8> = (0..4096u32).map(|i| (i % 251 + 1) as u8).collect();
    v.push(crc_vec("crc32-4k", "payload de 4 KB", &four_k, false));

    // ---------------- records (§6.4): uno de cada tipo ----------------
    v.push(rec_vec(
        "rec-session",
        "REC_SESSION tipado (draft.9) con tres cadenas PR-20 interiores",
        0,
        &Record::Session(Session {
            epoch: 7,
            catalog_hash: 0xDEAD_BEEF,
            chip_model: 9,
            chip_rev: 301,
            idf_ver: b"v5.3.1".to_vec(),
            app_name: b"bench".to_vec(),
            app_build_time: b"2026-08-18T12:00".to_vec(),
            app_elf_sha256: [1, 2, 3, 4, 5, 6, 7, 8],
            cpu_freq_mhz: 240,
            free_heap: 200_000,
            mole_ver: b"2.0.0".to_vec(),
        }),
        json!({"type": "REC_SESSION", "epoch": 7, "catalog_hash": 3735928559u32,
               "chip_model": 9, "chip_rev": 301, "idf_ver": "v5.3.1", "app_name": "bench",
               "app_build_time": "2026-08-18T12:00", "app_elf_sha256_hex": "0102030405060708",
               "cpu_freq_mhz": 240, "free_heap": 200000, "mole_ver": "2.0.0"}),
        false,
    ));
    v.push(rec_vec(
        "rec-sym-def",
        "catalogo CAT-04: sym 1, kind Tag(2), sin parent, nombre 'net'",
        0,
        &Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() },
        json!({"type": "REC_SYM_DEF", "sym_id": 1, "kind": 2, "parent": 0, "name": "net"}),
        true,
    ));
    v.push(rec_vec(
        "rec-stats",
        "REC_STATS completo (PR-13)",
        500,
        &Record::Stats(Stats {
            enqueued: 100_000,
            dropped: 1204,
            dropped_by_kind: [0, 1200, 4, 0, 0, 0, 0, 0],
            ring_high_water: 3800,
            sym_overflow: 0,
            tx_bytes: 9_999_999,
            free_heap: 181_000,
            min_free_heap: 152_000,
        }),
        json!({"type": "REC_STATS", "enqueued": 100000, "dropped": 1204,
               "dropped_by_kind": [0, 1200, 4, 0, 0, 0, 0, 0], "ring_high_water": 3800,
               "sym_overflow": 0, "tx_bytes": 9999999, "free_heap": 181000, "min_free_heap": 152000}),
        false,
    ));
    v.push(rec_vec(
        "rec-pong",
        "respuesta a CTL_PING",
        1,
        &Record::Pong { nonce: 0xCAFE_F00D },
        json!({"type": "REC_PONG", "nonce": 3405705229u32}),
        false,
    ));
    v.push(rec_vec(
        "rec-fmt-def",
        "REC-34: fmt_id 7, origen sym 4 linea 42, argc 2 (i32, f32)",
        0,
        &Record::FmtDef(FmtDef {
            fmt_id: 7,
            file_sym: 4,
            line: 42,
            arg_types: vec![ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)],
            fmt: b"crudo={} v={:.2}".to_vec(),
        }),
        json!({"type": "REC_FMT_DEF", "fmt_id": 7, "file_sym": 4, "line": 42,
               "argc": 2, "arg_types": ["i32", "f32"], "fmt": "crudo={} v={:.2}"}),
        true,
    ));
    v.push(rec_vec(
        "rec-fmt-def-struct-arg",
        "REC-44: arg_types con 0xF0 + type_id inline",
        0,
        &Record::FmtDef(FmtDef {
            fmt_id: 8,
            file_sym: 4,
            line: 77,
            arg_types: vec![ArgType::Struct(1), ArgType::Scalar(WireType::F32)],
            fmt: b"muestra {:#} umbral={:.1}".to_vec(),
        }),
        json!({"type": "REC_FMT_DEF", "fmt_id": 8, "file_sym": 4, "line": 77,
               "argc": 2, "arg_types": [{"struct": 1}, "f32"], "fmt": "muestra {:#} umbral={:.1}"}),
        false,
    ));
    for (id, t) in typedefs() {
        let anchor = id == "rec-type-def";
        let desc = match id.as_str() {
            "rec-type-def" => "REC-43 draft.9: descriptor con flags; el campo b es big-endian",
            "rec-type-def-padding-final" => "struct {u16;u8} con padding al final (sizeof 4, campos hasta el 3)",
            "rec-type-def-con-str" => "struct con cadena de runtime adentro: fuerza decodificacion secuencial (REC-46)",
            "rec-type-def-imu" => "draft.11: campo arreglo (flags bit 1, f32[3]) y campo enum (flags bit 2, ref_type 7)",
            _ => "descriptor auxiliar para vectores de structs",
        };
        v.push(rec_vec(&id, desc, 0, &Record::TypeDef(t.clone()), typedef_json(&t), anchor));
    }
    let me = mode_enum();
    v.push(rec_vec(
        "rec-enum-def",
        "REC-53 (draft.11): pares valor->nombre de Mode, base u8",
        0,
        &Record::EnumDef(me.clone()),
        json!({"type": "REC_ENUM_DEF", "type_id": 7, "name": "Mode", "wire": "u8",
               "nentries": 3,
               "entries": [{"value": 0, "name_sym": 40}, {"value": 1, "name_sym": 41},
                            {"value": 2, "name_sym": 42}]}),
        false,
    ));
    v.push(rec_vec(
        "rec-log",
        "REC_LOG fallback (REC-36), formateado en el MCU",
        9,
        &Record::Log {
            level: 3,
            task_id: 1,
            core: 0,
            tag_sym: 1,
            file_sym: 4,
            line: 55,
            msg: b"armado en runtime".to_vec(),
        },
        json!({"type": "REC_LOG", "level": 3, "task_id": 1, "core": 0, "tag_sym": 1,
               "file_sym": 4, "line": 55, "msg": "armado en runtime"}),
        false,
    ));
    v.push(rec_vec(
        "rec-log-fmt",
        "REC-35: INFO, tarea 3 core 1, tag 1, fmt 7, args (i32 1024, f32 2.47)",
        1500,
        &Record::LogFmt {
            level: 2,
            task_id: 3,
            core: 1,
            tag_sym: 1,
            fmt_id: 7,
            args_raw: unhex("000400007b141e40"),
        },
        json!({"type": "REC_LOG_FMT", "level": 2, "task_id": 3, "core": 1, "tag_sym": 1,
               "fmt_id": 7, "args": [{"t": "i32", "v": 1024}, {"t": "f32", "v": 2.47, "bits_hex": "7b141e40"}]}),
        true,
    ));
    v.push(rec_vec(
        "rec-watch",
        "REC-06: watch sym 5, f32 1.5",
        10,
        &Record::Watch { sym: 5, value: Value::F32(1.5) },
        json!({"type": "REC_WATCH", "sym": 5,
               "value": {"t": "f32", "v": 1.5, "bits_hex": "0000c03f"}}),
        true,
    ));
    v.push(rec_vec(
        "rec-watch-str",
        "watch de cadena",
        11,
        &Record::WatchStr { sym: 8, s: b"JOINED".to_vec() },
        json!({"type": "REC_WATCH_STR", "sym": 8, "s": "JOINED"}),
        false,
    ));
    v.push(rec_vec(
        "rec-bind-def-f32",
        "bind numerico: min/max/step del tama\u{f1}o del tipo (draft.10)",
        0,
        &Record::BindDef {
            sym: 9,
            ty: WireType::F32,
            flags: 0,
            min: Some(Value::F32(0.0)),
            max: Some(Value::F32(5.0)),
            step: Some(Value::F32(0.01)),
        },
        json!({"type": "REC_BIND_DEF", "sym": 9, "value_type": "f32", "flags": 0,
               "min": 0.0, "max": 5.0, "step": 0.01}),
        false,
    ));
    v.push(rec_vec(
        "rec-bind-def-bool",
        "bind bool: el payload termina en flags (draft.10)",
        0,
        &Record::BindDef { sym: 10, ty: WireType::Bool, flags: 0, min: None, max: None, step: None },
        json!({"type": "REC_BIND_DEF", "sym": 10, "value_type": "bool", "flags": 0}),
        false,
    ));
    v.push(rec_vec(
        "rec-bind-val",
        "reflejo bidireccional (REC-13)",
        2,
        &Record::BindVal { sym: 9, value: Value::F32(2.5) },
        json!({"type": "REC_BIND_VAL", "sym": 9, "value": {"t": "f32", "v": 2.5, "bits_hex": "00002040"}}),
        false,
    ));
    v.push(rec_vec(
        "rec-dump",
        "dump de 14 registros de un sensor I2C (REC-14)",
        77,
        &Record::Dump {
            sym: 11,
            schema_sym: 0,
            flags: 0,
            addr: 0x3B,
            data: unhex("0102030405060708090a0b0c0d0e"),
        },
        json!({"type": "REC_DUMP", "sym": 11, "schema_sym": 0, "flags": 0, "addr": 59,
               "data_hex": "0102030405060708090a0b0c0d0e"}),
        false,
    ));
    v.push(rec_vec(
        "rec-len-255",
        "payload de exactamente 255 bytes (maximo PR-07)",
        0,
        &Record::Dump { sym: 11, schema_sym: 0, flags: 0, addr: 0, data: vec![0x5A; 246] },
        json!({"type": "REC_DUMP", "sym": 11, "schema_sym": 0, "flags": 0, "addr": 0,
               "data_hex": hex(&[0x5A; 246])}),
        false,
    ));
    v.push(rec_vec(
        "rec-schema-def",
        "DSL de texto (REC-50, draft.10)",
        0,
        &Record::SchemaDef { sym: 12, def: b"u8 ver; u16 src; u8 flags:3;".to_vec() },
        json!({"type": "REC_SCHEMA_DEF", "sym": 12, "def": "u8 ver; u16 src; u8 flags:3;"}),
        false,
    ));
    v.push(rec_vec(
        "rec-span-begin",
        "span anidado con parent",
        3,
        &Record::SpanBegin { span_id: 100, sym: 6, parent_span_id: 99, task_id: 2 },
        json!({"type": "REC_SPAN_BEGIN", "span_id": 100, "sym": 6, "parent_span_id": 99, "task_id": 2}),
        false,
    ));
    v.push(rec_vec(
        "rec-span-end",
        "record minimo: header PR-07 + span_id",
        250,
        &Record::SpanEnd { span_id: 7 },
        json!({"type": "REC_SPAN_END", "span_id": 7}),
        true,
    ));
    v.push(rec_vec(
        "rec-span-end-dt-max",
        "dt_us en el maximo (PR-08)",
        65535,
        &Record::SpanEnd { span_id: 8 },
        json!({"type": "REC_SPAN_END", "span_id": 8}),
        false,
    ));
    v.push(rec_vec(
        "rec-span-abort",
        "REC-20 draft.9: reason PAUSED=1",
        4,
        &Record::SpanAbort { span_id: 100, reason: 1 },
        json!({"type": "REC_SPAN_ABORT", "span_id": 100, "reason": 1}),
        false,
    ));
    v.push(rec_vec(
        "rec-counter",
        "counters batcheados en un record (REC-23)",
        250,
        &Record::Counter { entries: vec![(13, 100_000), (14, 7)] },
        json!({"type": "REC_COUNTER", "n": 2,
               "entries": [{"sym": 13, "delta": 100000}, {"sym": 14, "delta": 7}]}),
        false,
    ));
    v.push(rec_vec(
        "rec-state",
        "transicion de maquina de estados (draft.9)",
        5,
        &Record::State { machine_sym: 15, state_sym: 16 },
        json!({"type": "REC_STATE", "machine_sym": 15, "state_sym": 16}),
        false,
    ));
    v.push(rec_vec(
        "rec-status",
        "LED de salud: AMARILLO(2) (draft.9)",
        6,
        &Record::Status { sym: 17, level: 2 },
        json!({"type": "REC_STATUS", "sym": 17, "level": 2}),
        false,
    ));
    v.push(rec_vec(
        "rec-check-fail",
        "check fallido con formateo diferido (draft.8): fmt_id + args crudos",
        7,
        &Record::CheckFail { fmt_id: 7, task_id: 1, core: 1, args_raw: unhex("000400007b141e40") },
        json!({"type": "REC_CHECK_FAIL", "fmt_id": 7, "task_id": 1, "core": 1,
               "args": [{"t": "i32", "v": 1024}, {"t": "f32", "v": 2.47, "bits_hex": "7b141e40"}]}),
        false,
    ));
    v.push(rec_vec(
        "rec-event",
        "marca puntual con argumento (FEAT-23)",
        8,
        &Record::Event { sym: 18, arg: 42 },
        json!({"type": "REC_EVENT", "sym": 18, "arg": 42}),
        false,
    ));
    v.push(rec_vec(
        "rec-cmd-def-noarg",
        "comando sin argumento: arg_type=0 (draft.10)",
        0,
        &Record::CmdDef { cmd_id: 1, sym: 19, arg_type: 0, min: None, max: None },
        json!({"type": "REC_CMD_DEF", "cmd_id": 1, "sym": 19, "arg_type": 0}),
        false,
    ));
    v.push(rec_vec(
        "rec-cmd-def-i32",
        "comando con argumento i32 y rango",
        0,
        &Record::CmdDef {
            cmd_id: 2,
            sym: 20,
            arg_type: WireType::I32.to_u8(),
            min: Some(Value::I32(0)),
            max: Some(Value::I32(15)),
        },
        json!({"type": "REC_CMD_DEF", "cmd_id": 2, "sym": 20, "arg_type": 6, "min": 0, "max": 15}),
        false,
    ));
    v.push(rec_vec(
        "rec-blob-begin",
        "inicio de blob fragmentado (REC-26)",
        0,
        &Record::BlobBegin { blob_id: 1, sym: 21, total_len: 100_000, schema_sym: 0 },
        json!({"type": "REC_BLOB_BEGIN", "blob_id": 1, "sym": 21, "total_len": 100000, "schema_sym": 0}),
        false,
    ));
    v.push(rec_vec(
        "rec-blob-chunk",
        "chunk con offset u32",
        1,
        &Record::BlobChunk { blob_id: 1, offset: 249, data: unhex("aabbccdd") },
        json!({"type": "REC_BLOB_CHUNK", "blob_id": 1, "offset": 249, "data_hex": "aabbccdd"}),
        false,
    ));
    v.push(rec_vec(
        "rec-paused",
        "checkpoint alcanzado (draft.9: file_sym + reason)",
        0,
        &Record::Paused { task_id: 2, file_sym: 4, line: 120, reason: 1 },
        json!({"type": "REC_PAUSED", "task_id": 2, "file_sym": 4, "line": 120, "reason": 1}),
        false,
    ));
    v.push(rec_vec(
        "rec-resumed",
        "reanudacion por timeout (draft.9: reason=3)",
        0,
        &Record::Resumed { task_id: 2, reason: 3 },
        json!({"type": "REC_RESUMED", "task_id": 2, "reason": 3}),
        false,
    ));
    // downlink (PR-15/PR-18): mismo framing
    v.push(rec_vec(
        "ctl-hello",
        "handshake CAT-09",
        0,
        &Record::CtlHello { proto_ver: 2, known_epoch: 0, known_catalog_hash: 0 },
        json!({"type": "CTL_HELLO", "proto_ver": 2, "known_epoch": 0, "known_catalog_hash": 0}),
        false,
    ));
    v.push(rec_vec(
        "ctl-cmd-i32",
        "invocacion de comando con argumento",
        0,
        &Record::CtlCmd { cmd_id: 2, arg_type: WireType::I32.to_u8(), arg: Some(Value::I32(11)) },
        json!({"type": "CTL_CMD", "cmd_id": 2, "arg_type": 6, "arg": 11}),
        false,
    ));
    v.push(rec_vec(
        "ctl-cmd-noarg",
        "invocacion de comando sin argumento",
        0,
        &Record::CtlCmd { cmd_id: 1, arg_type: 0, arg: None },
        json!({"type": "CTL_CMD", "cmd_id": 1, "arg_type": 0}),
        false,
    ));
    v.push(rec_vec(
        "ctl-bind-set",
        "escritura de bind desde el desktop",
        0,
        &Record::CtlBindSet { sym_id: 9, value: Value::F32(3.3) },
        json!({"type": "CTL_BIND_SET", "sym_id": 9,
               "value": {"t": "f32", "v": 3.3, "bits_hex": "33335340"}}),
        false,
    ));
    v.push(rec_vec(
        "ctl-pause",
        "pausa con scope All(1) (draft.9)",
        0,
        &Record::CtlPause { scope: 1, task_id: 0 },
        json!({"type": "CTL_PAUSE", "scope": 1, "task_id": 0}),
        false,
    ));
    v.push(rec_vec(
        "ctl-resume",
        "reanudar una tarea",
        0,
        &Record::CtlResume { scope: 0, task_id: 2 },
        json!({"type": "CTL_RESUME", "scope": 0, "task_id": 2}),
        false,
    ));
    v.push(rec_vec(
        "ctl-step",
        "paso hasta el proximo checkpoint",
        0,
        &Record::CtlStep { task_id: 2 },
        json!({"type": "CTL_STEP", "task_id": 2}),
        false,
    ));
    v.push(rec_vec(
        "ctl-reset",
        "reset con magic obligatorio (SEC-02)",
        0,
        &Record::CtlReset { magic: 0x4D4F_4C45 },
        json!({"type": "CTL_RESET", "magic": 1297434693u32}),
        false,
    ));
    v.push(rec_vec(
        "ctl-set-level",
        "nivel de log en vivo (PR-19)",
        0,
        &Record::CtlSetLevel { sym_id: 3, level: 1 },
        json!({"type": "CTL_SET_LEVEL", "sym_id": 3, "level": 1}),
        false,
    ));
    v.push(rec_vec(
        "ctl-set-policy",
        "backpressure en vivo: Decimate(3) (draft.9)",
        0,
        &Record::CtlSetPolicy { kind: 1, policy: 3 },
        json!({"type": "CTL_SET_POLICY", "kind": 1, "policy": 3}),
        false,
    ));
    v.push(rec_vec(
        "ctl-ping",
        "medicion de RTT",
        0,
        &Record::CtlPing { nonce: 1 },
        json!({"type": "CTL_PING", "nonce": 1}),
        false,
    ));

    // ---------------- args (§6.5) ----------------
    let no_reg = TypeRegistry::new();
    v.push(args_vec(
        "args-scalars-extremos",
        "cada entero en minimo/maximo/cero/negativo",
        &[
            ArgType::Scalar(WireType::I8), ArgType::Scalar(WireType::I8),
            ArgType::Scalar(WireType::U8), ArgType::Scalar(WireType::U8),
            ArgType::Scalar(WireType::I16), ArgType::Scalar(WireType::U16),
            ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::U32),
            ArgType::Scalar(WireType::I64), ArgType::Scalar(WireType::U64),
            ArgType::Scalar(WireType::Bool), ArgType::Scalar(WireType::Bool),
        ],
        &[
            Value::I8(-128), Value::I8(127), Value::U8(0), Value::U8(255),
            Value::I16(-32768), Value::U16(65535),
            Value::I32(i32::MIN), Value::U32(u32::MAX),
            Value::I64(i64::MIN), Value::U64(u64::MAX),
            Value::Bool(true), Value::Bool(false),
        ],
        json!([
            {"t": "i8", "v": -128}, {"t": "i8", "v": 127}, {"t": "u8", "v": 0}, {"t": "u8", "v": 255},
            {"t": "i16", "v": -32768}, {"t": "u16", "v": 65535},
            {"t": "i32", "v": -2147483648i64}, {"t": "u32", "v": 4294967295u32},
            {"t": "i64", "v": i64::MIN}, {"t": "u64", "v": u64::MAX},
            {"t": "bool", "v": true}, {"t": "bool", "v": false}
        ]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-floats-especiales",
        "NaN, +Inf, -Inf, -0.0 y subnormal, bits exactos",
        &[
            ArgType::Scalar(WireType::F32), ArgType::Scalar(WireType::F32),
            ArgType::Scalar(WireType::F32), ArgType::Scalar(WireType::F32),
            ArgType::Scalar(WireType::F32), ArgType::Scalar(WireType::F64),
            ArgType::Scalar(WireType::F64),
        ],
        &[
            Value::F32(f32::from_bits(0x7FC0_0001)),
            Value::F32(f32::INFINITY),
            Value::F32(f32::NEG_INFINITY),
            Value::F32(f32::from_bits(0x8000_0000)),
            Value::F32(f32::from_bits(0x0000_0001)),
            Value::F64(f64::from_bits(0x7FF8_0000_0000_0001)),
            Value::F64(f64::from_bits(0x0000_0000_0000_0001)),
        ],
        json!([
            {"t": "f32", "v": null, "bits_hex": "0100c07f"},
            {"t": "f32", "v": null, "bits_hex": "0000807f"},
            {"t": "f32", "v": null, "bits_hex": "000080ff"},
            {"t": "f32", "v": null, "bits_hex": "00000080"},
            {"t": "f32", "v": null, "bits_hex": "01000000"},
            {"t": "f64", "v": null, "bits_hex": "010000000000f87f"},
            {"t": "f64", "v": null, "bits_hex": "0100000000000000"}
        ]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-sym-literal",
        "cadena literal internada: 2 bytes para siempre (REC-38)",
        &[ArgType::Scalar(WireType::Sym)],
        &[Value::Sym(5)],
        json!([{"t": "sym", "v": 5}]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-str-vacia",
        "cadena de runtime vacia",
        &[ArgType::Scalar(WireType::Str)],
        &[Value::Str(vec![])],
        json!([{"t": "str", "v": ""}]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-str-1",
        "cadena de runtime de 1 byte",
        &[ArgType::Scalar(WireType::Str)],
        &[Value::Str(b"x".to_vec())],
        json!([{"t": "str", "v": "x"}]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-str-255",
        "cadena de runtime de 255 bytes (maximo PR-20)",
        &[ArgType::Scalar(WireType::Str)],
        &[Value::Str(vec![b'a'; 255])],
        json!([{"t": "str", "bytes_hex": hex(&[b'a'; 255])}]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-str-utf8-invalido",
        "UTF-8 invalido: el host hace from_utf8_lossy, nunca falla",
        &[ArgType::Scalar(WireType::Str)],
        &[Value::Str(unhex("ff6162"))],
        json!([{"t": "str", "bytes_hex": "ff6162"}]),
        &no_reg,
        &[],
        false,
    ));
    v.push(args_vec(
        "args-struct-values",
        "REC-45 modo valores: Sens{a=0x11, b=BE 0x2233}; sin padding, swap en el host",
        &[ArgType::Struct(1)],
        &[Value::Struct { type_id: 1, fields: vec![Value::U8(0x11), Value::U16(0x2233)] }],
        json!([{"t": "struct", "type_id": 1, "v": {"a": 17, "b": 8755, "_memoria_hex": "11002233"}}]),
        &reg_all,
        &["rec-type-def"],
        true,
    ));
    v.push(args_vec(
        "args-struct-anidado-2",
        "struct anidado a profundidad 2",
        &[ArgType::Struct(4)],
        &[Value::Struct {
            type_id: 4,
            fields: vec![
                Value::Struct { type_id: 3, fields: vec![Value::I16(-5)] },
                Value::F32(1.5),
            ],
        }],
        json!([{"t": "struct", "type_id": 4, "v": {"inner": {"val": -5}, "f": 1.5}}]),
        &reg_all,
        &["rec-type-def-interno", "rec-type-def-anidado"],
        false,
    ));
    v.push(args_vec(
        "args-struct-prof-4",
        "cadena de structs a profundidad 4, el maximo legal (REC-46)",
        &[ArgType::Struct(11)],
        &[Value::Struct {
            type_id: 11,
            fields: vec![Value::Struct {
                type_id: 12,
                fields: vec![Value::Struct {
                    type_id: 13,
                    fields: vec![Value::Struct { type_id: 14, fields: vec![Value::U8(0x42)] }],
                }],
            }],
        }],
        json!([{"t": "struct", "type_id": 11, "v": {"nested": 4, "leaf": 66}}]),
        &reg_all,
        &["rec-type-def-prof-1", "rec-type-def-prof-2", "rec-type-def-prof-3", "rec-type-def-prof-4"],
        false,
    ));
    v.push(args_vec(
        "args-struct-arreglo-enum",
        "draft.11: Imu{ax=f32[3], mode=Mode}; el arreglo empaqueta count elementos, el enum su entero base",
        &[ArgType::Struct(6)],
        &[Value::Struct {
            type_id: 6,
            fields: vec![
                Value::Array(vec![Value::F32(1.0), Value::F32(2.0), Value::F32(3.0)]),
                Value::Enum { type_id: 7, wire: WireType::U8, value: 1 },
            ],
        }],
        json!([{"t": "struct", "type_id": 6, "v": {"ax": [1.0, 2.0, 3.0], "mode": "AVG"}}]),
        &reg_all,
        &["rec-type-def-imu", "rec-enum-def"],
        false,
    ));
    v.push(args_vec(
        "args-enum-suelto",
        "draft.11: enum como argumento de log via tag 0xF1 + type_id inline (REC-53)",
        &[ArgType::Enum(7)],
        &[Value::Enum { type_id: 7, wire: WireType::U8, value: 1 }],
        json!([{"t": "enum", "type_id": 7, "v": 1}]),
        &reg_all,
        &["rec-enum-def"],
        false,
    ));
    v.push(args_vec(
        "args-struct-con-str",
        "struct con cadena de runtime adentro: decodificacion secuencial (REC-46)",
        &[ArgType::Struct(5)],
        &[Value::Struct {
            type_id: 5,
            fields: vec![Value::U8(5), Value::Str(b"abc".to_vec())],
        }],
        json!([{"t": "struct", "type_id": 5, "v": {"n": 5, "s": "abc"}}]),
        &reg_all,
        &["rec-type-def-con-str"],
        false,
    ));

    // ---------------- frames (§6.3) ----------------
    let span_end = Record::SpanEnd { span_id: 7 }.encode(250, &no_reg).unwrap();
    let watch = Record::Watch { sym: 5, value: Value::F32(1.5) }.encode(10, &no_reg).unwrap();
    let mut fr = frame_vec(
        "frame-two-records",
        "PR-02 completo: span-end + watch, CRC32 LE, COBS + 0x00",
        1,
        1_000_000,
        0,
        &[("rec-span-end", span_end.clone()), ("rec-watch", watch.clone())],
    );
    fr["anchor"] = json!(true);
    v.push(fr);
    v.push(frame_vec("frame-un-record", "frame con un solo record", 2, 1500, 0, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec("frame-vacio", "rec_count=0, legal", 0, 0, 0, &[]));
    v.push(frame_vec("frame-seq-65534", "anteultimo seq antes del wrap", 65534, 10, 0, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec("frame-seq-65535", "seq maximo; el siguiente es 0 (wrap)", 65535, 20, 0, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec("frame-flag-catalog", "FLAG_CATALOG solo", 3, 30, 0x1, &[("rec-sym-def", Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() }.encode(0, &no_reg).unwrap())]));
    v.push(frame_vec("frame-flag-drops", "FLAG_DROPS solo", 4, 40, 0x2, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec("frame-flag-paused", "FLAG_PAUSED solo", 5, 50, 0x4, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec("frame-flags-combinados", "los tres flags juntos", 6, 60, 0x7, &[("rec-span-end", span_end.clone())]));
    v.push(frame_vec(
        "frame-t-base-max",
        "t_base_us cerca del limite de 64 bits",
        7,
        u64::MAX - 15,
        0,
        &[("rec-span-end", span_end.clone())],
    ));
    let many: Vec<(&str, Vec<u8>)> = (0..200).map(|_| ("rec-span-end", span_end.clone())).collect();
    v.push(frame_vec("frame-rec-count-alto", "200 records en un frame", 8, 70, 0, &many));

    // ---------------- catalog (§6.4/§6.5) ----------------
    let cat_hash = {
        let mut cat = Catalog::new();
        cat.apply(&Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() }).unwrap();
        cat.apply(&Record::FmtDef(FmtDef {
            fmt_id: 7,
            file_sym: 4,
            line: 42,
            arg_types: vec![ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)],
            fmt: b"crudo={} v={:.2}".to_vec(),
        })).unwrap();
        cat.apply(&Record::TypeDef(typedefs()[0].1.clone())).unwrap();
        format!("{:08x}", cat.hash())
    };
    v.push(json!({
        "id": "cat-hash-basico",
        "layer": "catalog",
        "desc": "catalog_hash CAT-08 (draft.10): CRC32 incremental sobre payloads de definiciones, en orden",
        "records": ["rec-sym-def", "rec-fmt-def", "rec-type-def"],
        "expect": { "catalog_hash_hex": cat_hash, "sym_count": 1 }
    }));
    v.push(json!({
        "id": "cat-log-fmt-huerfano",
        "layer": "catalog",
        "desc": "REC_LOG_FMT cuyo REC_FMT_DEF nunca llego (§6.7 del plan)",
        "records": ["rec-log-fmt"],
        "must_fail": "unknown_fmt",
        "expect": { "error": "unknown_fmt" }
    }));

    // ---------------- must_fail (§6.7) ----------------
    let (_, wire_ok) = encode_frame(1, 1_000_000, 0, &[span_end.clone(), watch.clone()]).unwrap();
    let mut bad_crc = wire_ok.clone();
    // alterar un bit de un byte del medio sin introducir 0x00
    let mid = bad_crc.len() / 2;
    bad_crc[mid] ^= 0x01;
    if bad_crc[mid] == 0 {
        bad_crc[mid] = 0x02;
    }
    v.push(json!({
        "id": "fail-crc-un-bit", "layer": "frame", "must_fail": "crc_mismatch",
        "desc": "un bit alterado en el cuerpo: CRC32 no coincide",
        "wire_hex": hex(&bad_crc[..bad_crc.len() - 1]),
    }));
    v.push(json!({
        "id": "fail-cobs-cero-interno", "layer": "cobs", "must_fail": "cobs_invalid",
        "desc": "0x00 dentro del bloque COBS: es el delimitador",
        "encoded_hex": "021100",
    }));
    v.push(json!({
        "id": "fail-cobs-underrun", "layer": "cobs", "must_fail": "cobs_underrun",
        "desc": "el codigo promete mas bytes de los que hay",
        "encoded_hex": "0511",
    }));
    // len de record que excede el frame
    let bad_rec = vec![0x51u8, 0xFF, 0x00, 0x00, 0x07, 0x00];
    let mut body = vec![0x02u8];
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0u64.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&bad_rec);
    let crc = crc32(&body);
    body.extend_from_slice(&crc.to_le_bytes());
    let mut w = cobs::encode(&body);
    w.push(0);
    v.push(json!({
        "id": "fail-len-overflow", "layer": "frame", "must_fail": "len_overflow",
        "desc": "record con len=255 en un frame que no los tiene",
        "wire_hex": hex(&w[..w.len() - 1]),
    }));
    let (pre_ok, _) = encode_frame(1, 0, 0, &[span_end.clone()]).unwrap();
    let trunc = &pre_ok[..10];
    let mut wt2 = cobs::encode(trunc);
    wt2.push(0);
    v.push(json!({
        "id": "fail-truncado", "layer": "frame", "must_fail": "truncated",
        "desc": "frame mas corto que header + CRC",
        "wire_hex": hex(&wt2[..wt2.len() - 1]),
    }));
    v.push(rec_vec(
        "fail-tipo-desconocido",
        "type 0xEE: DEBE decodificar como Unknown, no fallar (§6.7)",
        0,
        &Record::Unknown { rtype: 0xEE, payload: vec![] },
        json!({"type": 238, "len": 0}),
        false,
    ));
    // marcar el must_fail a mano (rec_vec no lo pone)
    if let Some(last) = v.last_mut() {
        last["must_fail"] = json!("unknown_type");
    }
    v.push(json!({
        "id": "fail-args-count", "layer": "args", "must_fail": "arg_count_mismatch",
        "desc": "arg_types pide i32, hay 2 bytes",
        "arg_types_hex": "06", "bytes_hex": "0102",
    }));
    v.push(json!({
        "id": "fail-enum-sin-def", "layer": "args", "must_fail": "unknown_fmt",
        "desc": "tag 0xF1 con type_id que nunca se definio (draft.11)",
        "arg_types_hex": "f16300", "bytes_hex": "01",
    }));
    v.push(json!({
        "id": "fail-struct-prof-5", "layer": "args", "must_fail": "depth_exceeded",
        "desc": "struct anidado a profundidad 5 (REC-46: maximo 4)",
        "arg_types_hex": "f01500", "bytes_hex": "42",
        "requires": ["rec-type-def-exceso-1", "rec-type-def-exceso-2", "rec-type-def-exceso-3",
                      "rec-type-def-exceso-4", "rec-type-def-exceso-5"],
    }));

    v.sort_by(|a, b| a["id"].as_str().unwrap().cmp(b["id"].as_str().unwrap()));
    json!({
        "protocol_version": 2,
        "generated_by": "mole-vectors 0.1.0",
        "vectors": v,
    })
}

/// Reconstruye un TypeRegistry desde los vectores requeridos por id.
fn registry_from_requires(doc: &J, requires: &[&str]) -> TypeRegistry {
    let mut reg = TypeRegistry::new();
    let vectors = doc["vectors"].as_array().unwrap();
    for req in requires {
        let vec = vectors
            .iter()
            .find(|x| x["id"].as_str() == Some(req))
            .unwrap_or_else(|| panic!("requires apunta a vector inexistente: {req}"));
        let bytes = unhex(vec["bytes_hex"].as_str().unwrap());
        let payload = &bytes[4..];
        match Record::decode_payload(bytes[0], payload, &reg) {
            Ok(Record::TypeDef(t)) => reg.insert(t),
            Ok(Record::EnumDef(e)) => reg.insert_enum(e),
            _ => {}
        }
    }
    reg
}

/// Valida el archivo completo contra el codec. Devuelve la lista de fallos.
pub fn validate(doc: &J) -> Vec<String> {
    let mut fails = Vec::new();
    let vectors = match doc["vectors"].as_array() {
        Some(v) => v,
        None => return vec!["sin arreglo vectors".into()],
    };
    let mut fail = |id: &str, msg: String| fails.push(format!("{id}: {msg}"));

    for vec in vectors {
        let id = vec["id"].as_str().unwrap_or("<sin id>");
        let layer = vec["layer"].as_str().unwrap_or("");
        let must_fail = vec.get("must_fail").and_then(|m| m.as_str());
        match (layer, must_fail) {
            ("cobs", None) => {
                let dec = unhex(vec["decoded_hex"].as_str().unwrap());
                let enc = unhex(vec["encoded_hex"].as_str().unwrap());
                if cobs::encode(&dec) != enc {
                    fail(id, "encode COBS difiere".into());
                }
                match cobs::decode(&enc) {
                    Ok(d) if d == dec => {}
                    _ => fail(id, "decode COBS difiere".into()),
                }
            }
            ("cobs", Some(reason)) => {
                let enc = unhex(vec["encoded_hex"].as_str().unwrap());
                match cobs::decode(&enc) {
                    Err(e) if e.as_must_fail() == reason => {}
                    other => fail(id, format!("esperaba {reason}, obtuve {other:?}")),
                }
            }
            ("crc32", _) => {
                let data = unhex(vec["data_hex"].as_str().unwrap());
                let expected = vec["crc_hex"].as_str().unwrap();
                let got = format!("{:08x}", crc32(&data));
                if got != expected {
                    fail(id, format!("crc {got} != {expected}"));
                }
            }
            ("record", mf) => {
                let bytes = unhex(vec["bytes_hex"].as_str().unwrap());
                if bytes.len() < 4 {
                    fail(id, "record mas corto que su header".into());
                    continue;
                }
                let (rtype, len) = (bytes[0], bytes[1] as usize);
                let dt = u16::from_le_bytes([bytes[2], bytes[3]]);
                if bytes.len() != 4 + len {
                    fail(id, "len del header no coincide con bytes_hex".into());
                    continue;
                }
                let reg = TypeRegistry::new();
                match Record::decode_payload(rtype, &bytes[4..], &reg) {
                    Ok(rec) => {
                        if mf == Some("unknown_type") && !matches!(rec, Record::Unknown { .. }) {
                            fail(id, "debia decodificar como Unknown".into());
                        }
                        match rec.encode(dt, &reg) {
                            Ok(re) if re == bytes => {}
                            _ => fail(id, "re-encode difiere del original".into()),
                        }
                    }
                    Err(e) => fail(id, format!("decode fallo: {e}")),
                }
            }
            ("args", mf) => {
                let th = unhex(vec["arg_types_hex"].as_str().unwrap());
                let bytes = unhex(vec["bytes_hex"].as_str().unwrap());
                let reqs: Vec<&str> = vec
                    .get("requires")
                    .and_then(|r| r.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                let reg = registry_from_requires(doc, &reqs);
                let mut r = mole_codec::rw::Reader::new(&th);
                let n_args = count_arg_types(&th);
                let types = match decode_arg_types(&mut r, n_args) {
                    Ok(t) => t,
                    Err(e) => {
                        fail(id, format!("arg_types_hex invalido: {e}"));
                        continue;
                    }
                };
                match (decode_args(&types, &bytes, &reg), mf) {
                    (Ok(vals), None) => match encode_args(&vals, &reg) {
                        Ok(re) if re == bytes => {}
                        _ => fail(id, "re-encode de args difiere".into()),
                    },
                    (Err(e), Some(reason)) if e.as_must_fail() == reason => {}
                    (got, mf) => fail(id, format!("esperaba {mf:?}, obtuve {got:?}")),
                }
            }
            ("frame", None) => {
                let wire = unhex(vec["wire_hex"].as_str().unwrap());
                let pre = unhex(vec["pre_cobs_hex"].as_str().unwrap());
                let f = &vec["frame"];
                let refs: Vec<Vec<u8>> = f["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|rid| {
                        let rid = rid.as_str().unwrap();
                        let rv = vectors.iter().find(|x| x["id"].as_str() == Some(rid)).unwrap();
                        unhex(rv["bytes_hex"].as_str().unwrap())
                    })
                    .collect();
                let mut flags = 0u8;
                for name in f["flags"].as_array().unwrap() {
                    flags |= match name.as_str().unwrap() {
                        "CATALOG" => 0x1,
                        "DROPS" => 0x2,
                        "PAUSED" => 0x4,
                        _ => 0,
                    };
                }
                let (p, w) = encode_frame(
                    f["seq"].as_u64().unwrap() as u16,
                    f["t_base_us"].as_u64().unwrap(),
                    flags,
                    &refs,
                )
                .unwrap();
                if p != pre {
                    fail(id, "pre_cobs difiere".into());
                }
                if w[..w.len() - 1] != wire[..wire.len() - 1] || *wire.last().unwrap() != 0 {
                    fail(id, "wire difiere".into());
                }
                if decode_wire(&wire[..wire.len() - 1]).is_err() {
                    fail(id, "decode del propio wire fallo".into());
                }
            }
            ("frame", Some(reason)) => {
                let wire = unhex(vec["wire_hex"].as_str().unwrap());
                match decode_wire(&wire) {
                    Err(e) if e.as_must_fail() == reason => {}
                    other => fail(id, format!("esperaba {reason}, obtuve {other:?}")),
                }
            }
            ("catalog", mf) => {
                let mut cat = Catalog::new();
                let mut orphan_err: Option<DecodeError> = None;
                for rid in vec["records"].as_array().unwrap() {
                    let rid = rid.as_str().unwrap();
                    let rv = vectors.iter().find(|x| x["id"].as_str() == Some(rid)).unwrap();
                    let bytes = unhex(rv["bytes_hex"].as_str().unwrap());
                    let rec = Record::decode_payload(bytes[0], &bytes[4..], &cat.types)
                        .unwrap_or_else(|e| panic!("decode de {rid} en {id}: {e}"));
                    cat.apply(&rec).unwrap();
                    if let Record::LogFmt { fmt_id, args_raw, .. } = &rec {
                        if let Err(e) = cat.parse_args(*fmt_id, args_raw) {
                            orphan_err = Some(e);
                        }
                    }
                }
                match (mf, orphan_err) {
                    (Some(reason), Some(e)) if e.as_must_fail() == reason => {}
                    (Some(reason), other) => fail(id, format!("esperaba {reason}, obtuve {other:?}")),
                    (None, _) => {
                        let expect = &vec["expect"];
                        if let Some(h) = expect.get("catalog_hash_hex").and_then(|x| x.as_str()) {
                            let got = format!("{:08x}", cat.hash());
                            if got != h {
                                fail(id, format!("catalog_hash {got} != {h}"));
                            }
                        }
                        if let Some(n) = expect.get("sym_count").and_then(|x| x.as_u64()) {
                            if cat.sym_count() as u64 != n {
                                fail(id, "sym_count difiere".into());
                            }
                        }
                    }
                }
            }
            other => fail(id, format!("capa no manejada: {other:?}")),
        }
    }
    fails
}

/// Cuenta cuántos args describe un `arg_types_hex` (0xF0/0xF1 consumen 2 extra).
fn count_arg_types(th: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < th.len() {
        n += 1;
        i += if th[i] == 0xF0 || th[i] == 0xF1 { 3 } else { 1 };
    }
    n
}
