// SPDX-License-Identifier: MIT
// mole_codec.h — COBS + CRC32 + armado y parseo de frames (spec §6).
// Compila en host (g++/CMake) y en ESP32. Sin Arduino.h, sin ESP-IDF,
// sin asignación dinámica (C-4): el llamador provee todos los buffers.
#pragma once

#include "mole_wire.h"

namespace mole {

// Resultado de decodificación. Espejo del enum must_fail de los vectores.
enum class Err : uint8_t {
  Ok = 0,
  CrcMismatch,
  CobsInvalid,
  CobsUnderrun,
  LenOverflow,
  Truncated,
  BadVersion,
};

const char* err_name(Err e);  // nombre must_fail ("crc_mismatch", ...)

// ---------------------------------------------------------------------------
// COBS — §6.1. Convención del borde 254 fijada por el ancla cobs-254:
// el encoder emite el grupo final vacío; el decoder acepta ambas variantes.
// ---------------------------------------------------------------------------

// Peor caso de expansión COBS para n bytes (sin el delimitador).
inline constexpr size_t cobs_max_encoded(size_t n) { return n + n / 254 + 2; }

// Codifica src[0..n) en dst. Devuelve la longitud escrita, o 0 si no entra
// (cap insuficiente). La salida no contiene 0x00 ni delimitador.
size_t cobs_encode(uint8_t* dst, size_t cap, const uint8_t* src, size_t n);

// Decodifica un bloque COBS (sin delimitador). Escribe en dst y deja la
// longitud en *out_n.
Err cobs_decode(uint8_t* dst, size_t cap, size_t* out_n, const uint8_t* src, size_t n);

// ---------------------------------------------------------------------------
// CRC32 — PR-03. CRC-32/ISO-HDLC (poly zlib), convención Q-1 fijada:
// crc32(buf) == ~esp_rom_crc32_le(~0, buf, len). Check("123456789")=0xCBF43926.
// ---------------------------------------------------------------------------

uint32_t crc32(const uint8_t* data, size_t n);
// Variante incremental para el catalog_hash (CAT-08): state arranca en
// 0xFFFFFFFF, se alimenta y el valor final es state ^ 0xFFFFFFFF.
uint32_t crc32_update(uint32_t state, const uint8_t* data, size_t n);

// ---------------------------------------------------------------------------
// Armado de frames — PR-02/PR-05. El llamador provee el buffer (típicamente
// de MOLE_FRAME_MAX) y agrega records ya serializados o por partes.
// ---------------------------------------------------------------------------

struct FrameWriter {
  uint8_t* buf = nullptr;
  size_t cap = 0;
  size_t len = 0;
  uint16_t rec_count = 0;
  bool overflow = false;
};

// Escribe el header con rec_count=0 (se corrige en fw_end).
void fw_begin(FrameWriter& w, uint8_t* buf, size_t cap, uint16_t seq,
              uint64_t t_base_us, uint8_t flags);
// Agrega un record completo (header PR-07 + payload). false si no entra.
bool fw_add_record(FrameWriter& w, uint8_t type, uint16_t dt_us,
                   const uint8_t* payload, size_t n);
// Cierra: corrige rec_count y agrega CRC32. Devuelve la longitud pre-COBS,
// o 0 si hubo overflow en algún paso.
size_t fw_end(FrameWriter& w);

// ---------------------------------------------------------------------------
// Parseo de frames (downlink en el MCU; también lo usa el runner de tests).
// ---------------------------------------------------------------------------

struct FrameHeader {
  uint8_t ver = 0;
  uint8_t flags = 0;  // nibble alto de ver_flags, ya desplazado
  uint16_t seq = 0;
  uint64_t t_base_us = 0;
  uint16_t rec_count = 0;
};

struct RecordView {
  uint8_t type = 0;
  uint16_t dt_us = 0;
  const uint8_t* payload = nullptr;
  uint8_t len = 0;
};

// Verifica CRC y header de un frame pre-COBS. No copia nada.
Err frame_parse_header(const uint8_t* pre_cobs, size_t n, FrameHeader* h);

// Itera los records de un frame ya verificado con frame_parse_header.
// Escribe hasta max vistas en recs y deja la cantidad en *out_count.
// PR-07: un len que excede el frame es LenOverflow, igual que bytes sobrantes.
Err frame_records(const uint8_t* pre_cobs, size_t n, RecordView* recs,
                  size_t max, size_t* out_count);

// ---------------------------------------------------------------------------
// Escritura little-endian de payloads (para la API de F1 y los tests).
// ---------------------------------------------------------------------------

inline void put_u16(uint8_t* p, uint16_t v) {
  p[0] = static_cast<uint8_t>(v);
  p[1] = static_cast<uint8_t>(v >> 8);
}
inline void put_u32(uint8_t* p, uint32_t v) {
  put_u16(p, static_cast<uint16_t>(v));
  put_u16(p + 2, static_cast<uint16_t>(v >> 16));
}
inline void put_u64(uint8_t* p, uint64_t v) {
  put_u32(p, static_cast<uint32_t>(v));
  put_u32(p + 4, static_cast<uint32_t>(v >> 32));
}
inline uint16_t get_u16(const uint8_t* p) {
  return static_cast<uint16_t>(p[0] | (p[1] << 8));
}
inline uint32_t get_u32(const uint8_t* p) {
  return static_cast<uint32_t>(get_u16(p)) | (static_cast<uint32_t>(get_u16(p + 2)) << 16);
}
inline uint64_t get_u64(const uint8_t* p) {
  return static_cast<uint64_t>(get_u32(p)) | (static_cast<uint64_t>(get_u32(p + 4)) << 32);
}

}  // namespace mole
