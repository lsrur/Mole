//! C-6: los tests de Rust leen el MISMO codec_vectors.json commiteado que
//! consume el runner de C++. Además, el archivo debe ser exactamente lo que
//! `build()` genera: cualquier drift aparece acá y en el diff del PR (V-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

fn committed() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/codec_vectors.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("falta codec_vectors.json — correr `mole-vectors generate`")).unwrap()
}

#[test]
fn el_archivo_commiteado_valida_contra_el_codec() {
    let fails = mole_vectors::validate(&committed());
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

#[test]
fn el_archivo_commiteado_es_el_generado() {
    let built = mole_vectors::build();
    let disk = committed();
    assert_eq!(
        built, disk,
        "codec_vectors.json difiere de lo que genera build(): regenerar con `mole-vectors generate` y revisar el diff"
    );
}

#[test]
fn ids_unicos_y_ordenados() {
    let doc = committed();
    let ids: Vec<&str> = doc["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(ids, sorted, "los vectores deben estar ordenados por id y sin duplicados (V-6)");
}
