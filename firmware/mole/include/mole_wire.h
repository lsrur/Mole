// SPDX-License-Identifier: MIT
// mole_wire.h — opcodes, layouts y constantes del protocolo Mole v2.
// Solo declaraciones, sin lógica (R-3). Autoridad: specs/mole-spec.md
// §6/§7, congeladas en draft.10. Espejo de host/mole-codec/src/wire.rs.
#pragma once

#include <stddef.h>
#include <stdint.h>

namespace mole {

// PR-02: bits 0-3 de ver_flags.
inline constexpr uint8_t kProtocolVersion = 2;
// PR-04: tamaño máximo de frame pre-COBS.
inline constexpr size_t kFrameMax = 4096;
// PR-02: ver_flags + seq + t_base_us + rec_count.
inline constexpr size_t kFrameHeaderLen = 1 + 2 + 8 + 2;
// PR-07: type + len + dt_us.
inline constexpr size_t kRecordHeaderLen = 4;
// PR-18 / SEC-02: magic de CTL_RESET ("MOLE").
inline constexpr uint32_t kResetMagic = 0x4D4F4C45;
// REC-46: profundidad máxima de structs anidados.
inline constexpr size_t kMaxStructDepth = 4;
// CAT-05 / CAT-06.
inline constexpr uint16_t kSymNone = 0;
inline constexpr uint16_t kSymOverflow = 0xFFFF;

// Tipos de record, uplink — §7.1.
enum RecType : uint8_t {
  REC_SESSION = 0x01,
  REC_SYM_DEF = 0x02,
  REC_STATS = 0x03,
  REC_PONG = 0x04,
  REC_FMT_DEF = 0x05,
  REC_TYPE_DEF = 0x06,
  REC_LOG = 0x10,
  REC_LOG_FMT = 0x11,
  REC_WATCH = 0x20,
  REC_WATCH_STR = 0x21,
  REC_BIND_DEF = 0x30,
  REC_BIND_VAL = 0x31,
  REC_DUMP = 0x40,
  REC_SCHEMA_DEF = 0x41,
  REC_SPAN_BEGIN = 0x50,
  REC_SPAN_END = 0x51,
  REC_SPAN_ABORT = 0x52,
  REC_COUNTER = 0x60,
  REC_STATE = 0x70,
  REC_STATUS = 0x71,
  REC_CHECK_FAIL = 0x80,
  REC_EVENT = 0x81,
  REC_CMD_DEF = 0x90,
  REC_BLOB_BEGIN = 0xA0,
  REC_BLOB_CHUNK = 0xA1,
  REC_PAUSED = 0xF0,
  REC_RESUMED = 0xF1,
  // Downlink — PR-18 (draft.10).
  CTL_HELLO = 0xC0,
  CTL_CMD = 0xC1,
  CTL_BIND_SET = 0xC2,
  CTL_PAUSE = 0xC3,
  CTL_RESUME = 0xC4,
  CTL_STEP = 0xC5,
  CTL_RESET = 0xC6,
  CTL_SET_LEVEL = 0xC7,
  CTL_SET_POLICY = 0xC8,
  CTL_PING = 0xC9,
};

// Flags de frame (PR-02), normalizados al nibble bajo.
enum FrameFlag : uint8_t {
  FLAG_CATALOG = 1 << 0,
  FLAG_DROPS = 1 << 1,
  FLAG_PAUSED = 1 << 2,
};

// Enum de tipos de wire — REC-52. Un solo enum para args, watch, bind y
// descriptores.
enum WireType : uint8_t {
  WIRE_NONE = 0x00,  // reservado; en REC_CMD_DEF significa "sin argumento"
  WIRE_U8 = 0x01,
  WIRE_I8 = 0x02,
  WIRE_U16 = 0x03,
  WIRE_I16 = 0x04,
  WIRE_U32 = 0x05,
  WIRE_I32 = 0x06,
  WIRE_U64 = 0x07,
  WIRE_I64 = 0x08,
  WIRE_F32 = 0x09,
  WIRE_F64 = 0x0A,
  WIRE_BOOL = 0x0B,
  WIRE_SYM = 0x0C,  // cadena literal internada: u16 sym_id
  WIRE_STR = 0x0D,  // cadena de runtime: prefijo u8 (PR-20)
  WIRE_PTR = 0x0E,  // dirección de 4 bytes
  WIRE_STRUCT = 0xF0,
};

// Tamaño fijo en el wire; 0 para tipos variables (Str/Struct) o inválidos.
inline constexpr size_t wire_fixed_size(uint8_t t) {
  switch (t) {
    case WIRE_U8:
    case WIRE_I8:
    case WIRE_BOOL:
      return 1;
    case WIRE_U16:
    case WIRE_I16:
    case WIRE_SYM:
      return 2;
    case WIRE_U32:
    case WIRE_I32:
    case WIRE_F32:
    case WIRE_PTR:
      return 4;
    case WIRE_U64:
    case WIRE_I64:
    case WIRE_F64:
      return 8;
    default:
      return 0;
  }
}

// Tipos de símbolo — CAT-03.
enum SymKind : uint8_t {
  KIND_WATCH = 1,
  KIND_TAG = 2,
  KIND_TASK = 3,
  KIND_FILE = 4,
  KIND_COMMAND = 5,
  KIND_SPAN = 6,
  KIND_COUNTER = 7,
  KIND_MACHINE = 8,
  KIND_STATE = 9,
  KIND_BIND = 10,
  KIND_DUMP = 11,
  KIND_SCHEMA = 12,
};

// Bit 0 de flags por campo en REC_TYPE_DEF (draft.9): big-endian.
inline constexpr uint8_t kFieldFlagBE = 0x01;

}  // namespace mole
