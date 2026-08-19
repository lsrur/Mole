//! Codec del protocolo Mole v2 (spec §6/§7, congeladas en draft.8).
//!
//! Entrada no confiable: el camino de decodificación devuelve `Result`,
//! nunca paniquea (SEC-04, C-3).

pub mod args;
pub mod catalog;
pub mod cobs;
pub mod crc32;
pub mod error;
pub mod frame;
pub mod record;
pub mod rw;
pub mod types;
pub mod wire;

pub use error::DecodeError;

/// Versión de protocolo (spec PR-02, bits 0-3 de `ver_flags`).
pub const PROTOCOL_VERSION: u8 = 2;
