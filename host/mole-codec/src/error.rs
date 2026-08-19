//! Errores de decodificación. Entrada no confiable (SEC-04): nada acá paniquea.

use core::fmt;

/// Motivos de rechazo. Espejo del enum `must_fail` de codec_vectors.schema.json.
///
/// Un `type` de record desconocido NO es un error: decodifica como
/// `Record::Unknown` (matriz §6.7 del plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// CRC32 del frame no coincide (PR-03).
    CrcMismatch,
    /// COBS con byte 0x00 interno: es el delimitador, no puede aparecer adentro.
    CobsInvalid,
    /// COBS truncado: el código promete más bytes de los que hay.
    CobsUnderrun,
    /// El `len` de un record excede lo que queda del frame (PR-07).
    LenOverflow,
    /// Frame más corto que su header + CRC.
    Truncated,
    /// Cantidad de argumentos en desacuerdo con el `REC_FMT_DEF` (REC-35).
    ArgCountMismatch,
    /// Struct anidado más allá de profundidad 4 (REC-46).
    DepthExceeded,
    /// `REC_LOG_FMT` cuyo `REC_FMT_DEF` nunca llegó.
    UnknownFmt,
    /// Versión de protocolo distinta de 2 (PR-02).
    BadVersion,
}

impl DecodeError {
    /// Nombre del motivo tal como aparece en `must_fail` de los vectores.
    pub fn as_must_fail(&self) -> &'static str {
        match self {
            DecodeError::CrcMismatch => "crc_mismatch",
            DecodeError::CobsInvalid => "cobs_invalid",
            DecodeError::CobsUnderrun => "cobs_underrun",
            DecodeError::LenOverflow => "len_overflow",
            DecodeError::Truncated => "truncated",
            DecodeError::ArgCountMismatch => "arg_count_mismatch",
            DecodeError::DepthExceeded => "depth_exceeded",
            DecodeError::UnknownFmt => "unknown_fmt",
            DecodeError::BadVersion => "bad_version",
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_must_fail())
    }
}

impl std::error::Error for DecodeError {}
