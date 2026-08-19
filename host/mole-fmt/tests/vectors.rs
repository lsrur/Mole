//! TEST-08: el formateador contra protocol/fmt_vectors.json.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::Path;

use mole_codec::args::Value;
use mole_codec::types::{FieldDef, TypeDef};
use mole_codec::wire::WireType;
use mole_fmt::{format, Resolver};
use serde_json::Value as Json;

struct MapResolver {
    syms: HashMap<u16, String>,
    types: HashMap<u16, TypeDef>,
    enums: HashMap<u16, HashMap<i64, String>>,
}

impl Resolver for MapResolver {
    fn sym_name(&self, sym_id: u16) -> Option<String> {
        self.syms.get(&sym_id).cloned()
    }
    fn type_def(&self, type_id: u16) -> Option<&TypeDef> {
        self.types.get(&type_id)
    }
    fn enum_value_name(&self, type_id: u16, value: i64) -> Option<String> {
        self.enums.get(&type_id)?.get(&value).cloned()
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn parse_arg(j: &Json) -> Value {
    let t = j["t"].as_str().unwrap();
    match t {
        "u8" => Value::U8(j["v"].as_u64().unwrap() as u8),
        "i8" => Value::I8(j["v"].as_i64().unwrap() as i8),
        "u16" => Value::U16(j["v"].as_u64().unwrap() as u16),
        "i16" => Value::I16(j["v"].as_i64().unwrap() as i16),
        "u32" => Value::U32(j["v"].as_u64().unwrap() as u32),
        "i32" => Value::I32(j["v"].as_i64().unwrap() as i32),
        "u64" => Value::U64(j["v"].as_u64().unwrap()),
        "i64" => Value::I64(j["v"].as_i64().unwrap()),
        "f32" => {
            let b = unhex(j["bits_hex"].as_str().expect("f32 exige bits_hex"));
            Value::F32(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        }
        "f64" => {
            let b = unhex(j["bits_hex"].as_str().expect("f64 exige bits_hex"));
            Value::F64(f64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]))
        }
        "bool" => Value::Bool(j["v"].as_bool().unwrap()),
        "sym" => Value::Sym(j["v"].as_u64().unwrap() as u16),
        "str" => match j.get("bytes_hex") {
            Some(h) => Value::Str(unhex(h.as_str().unwrap())),
            None => Value::Str(j["v"].as_str().unwrap().as_bytes().to_vec()),
        },
        "ptr" => Value::Ptr(j["v"].as_u64().unwrap() as u32),
        "struct" => Value::Struct {
            type_id: j["type_id"].as_u64().unwrap() as u16,
            fields: j["fields"].as_array().unwrap().iter().map(parse_arg).collect(),
        },
        "enum" => Value::Enum {
            type_id: j["type_id"].as_u64().unwrap() as u16,
            wire: match j["wire"].as_str().unwrap() {
                "u8" => WireType::U8,
                "i32" => WireType::I32,
                other => panic!("base de enum {other} no soportada en fmt_vectors"),
            },
            value: j["v"].as_i64().unwrap(),
        },
        "array" => Value::Array(j["elems"].as_array().unwrap().iter().map(parse_arg).collect()),
        other => panic!("tipo de arg desconocido en vector: {other}"),
    }
}

fn parse_resolver(j: &Json) -> MapResolver {
    let mut syms = HashMap::new();
    if let Some(map) = j.get("syms").and_then(|s| s.as_object()) {
        for (k, v) in map {
            syms.insert(k.parse::<u16>().unwrap(), v.as_str().unwrap().to_string());
        }
    }
    let mut types = HashMap::new();
    if let Some(arr) = j.get("types").and_then(|t| t.as_array()) {
        for t in arr {
            let type_id = t["type_id"].as_u64().unwrap() as u16;
            let fields = t["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| FieldDef {
                    name_sym: f["name_sym"].as_u64().unwrap() as u16,
                    wire: match f["wire"].as_str().unwrap() {
                        "u8" => WireType::U8,
                        "u16" => WireType::U16,
                        "i16" => WireType::I16,
                        "u32" => WireType::U32,
                        "i32" => WireType::I32,
                        "f32" => WireType::F32,
                        "bool" => WireType::Bool,
                        other => panic!("wire {other} no soportado en fmt_vectors"),
                    },
                    flags: 0,
                    offset: 0,
                    size: 0,
                    ref_type: 0,
                })
                .collect();
            types.insert(
                type_id,
                TypeDef {
                    type_id,
                    name: t["name"].as_str().unwrap().as_bytes().to_vec(),
                    fields,
                },
            );
        }
    }
    let mut enums = HashMap::new();
    if let Some(map) = j.get("enums").and_then(|s| s.as_object()) {
        for (type_id, entries) in map {
            let mut inner = HashMap::new();
            for (value, name) in entries.as_object().unwrap() {
                inner.insert(value.parse::<i64>().unwrap(), name.as_str().unwrap().to_string());
            }
            enums.insert(type_id.parse::<u16>().unwrap(), inner);
        }
    }
    MapResolver { syms, types, enums }
}

#[test]
fn vectores_test_08() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/fmt_vectors.json");
    let doc: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut failures = Vec::new();
    for v in doc["vectors"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let fmt = v["fmt"].as_str().unwrap();
        let args: Vec<Value> = v["args"].as_array().unwrap().iter().map(parse_arg).collect();
        let res = parse_resolver(v);
        let got = format(fmt, &args, &res);
        let expected = v["expected"].as_str().unwrap();
        if got != expected {
            failures.push(format!("{id}: esperado {expected:?}, obtenido {got:?}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
