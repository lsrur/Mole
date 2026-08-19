//! CLI: `mole-vectors generate [ruta]` escribe codec_vectors.json;
//! `mole-vectors validate [ruta]` lo valida contra el codec (V-7: CI valida,
//! no regenera).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("validate");
    let default_path = "protocol/codec_vectors.json".to_string();
    let path = args.get(2).unwrap_or(&default_path);

    match mode {
        "generate" => {
            let doc = mole_vectors::build();
            let fails = mole_vectors::validate(&doc);
            if !fails.is_empty() {
                eprintln!("los vectores generados no validan contra el propio codec:");
                for f in &fails {
                    eprintln!("  {f}");
                }
                return ExitCode::FAILURE;
            }
            let mut out = serde_json::to_string_pretty(&doc).unwrap_or_default();
            out.push('\n');
            if let Err(e) = std::fs::write(path, out) {
                eprintln!("no pude escribir {path}: {e}");
                return ExitCode::FAILURE;
            }
            println!("{path} escrito y validado");
            ExitCode::SUCCESS
        }
        "validate" => {
            let raw = match std::fs::read_to_string(path) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("no pude leer {path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let doc: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{path} no es JSON: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let fails = mole_vectors::validate(&doc);
            if fails.is_empty() {
                let n = doc["vectors"].as_array().map(Vec::len).unwrap_or(0);
                println!("{n} vectores validados contra el codec de Rust");
                ExitCode::SUCCESS
            } else {
                for f in &fails {
                    eprintln!("FALLO {f}");
                }
                ExitCode::FAILURE
            }
        }
        other => {
            eprintln!("modo desconocido: {other} (use generate|validate)");
            ExitCode::FAILURE
        }
    }
}
