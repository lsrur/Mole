// SPDX-License-Identifier: MIT
// mole_codec.cpp — implementación de la capa de codificación (spec §6).

#include "mole_codec.h"

namespace mole {

const char* err_name(Err e) {
  switch (e) {
    case Err::Ok:
      return "ok";
    case Err::CrcMismatch:
      return "crc_mismatch";
    case Err::CobsInvalid:
      return "cobs_invalid";
    case Err::CobsUnderrun:
      return "cobs_underrun";
    case Err::LenOverflow:
      return "len_overflow";
    case Err::Truncated:
      return "truncated";
    case Err::BadVersion:
      return "bad_version";
  }
  return "?";
}

// ---------------------------------------------------------------------------
// COBS
// ---------------------------------------------------------------------------

size_t cobs_encode(uint8_t* dst, size_t cap, const uint8_t* src, size_t n) {
  size_t out = 0;
  size_t code_idx = 0;
  if (cap < 1) return 0;
  dst[out++] = 0;
  uint8_t code = 1;
  for (size_t i = 0; i < n; i++) {
    const uint8_t b = src[i];
    if (b != 0) {
      if (out >= cap) return 0;
      dst[out++] = b;
      code++;
    }
    if (b == 0 || code == 0xFF) {
      dst[code_idx] = code;
      code_idx = out;
      if (out >= cap) return 0;
      dst[out++] = 0;
      code = 1;
    }
  }
  dst[code_idx] = code;
  return out;
}

Err cobs_decode(uint8_t* dst, size_t cap, size_t* out_n, const uint8_t* src, size_t n) {
  *out_n = 0;
  if (n == 0) return Err::CobsUnderrun;
  size_t out = 0;
  size_t i = 0;
  while (i < n) {
    const uint8_t code = src[i];
    if (code == 0) return Err::CobsInvalid;
    i++;
    const size_t grp = static_cast<size_t>(code) - 1;
    if (i + grp > n) return Err::CobsUnderrun;
    for (size_t k = 0; k < grp; k++) {
      // El delimitador no puede aparecer en ninguna posición: el splitter
      // de frames ya cortó en 0x00, así que un 0x00 acá es corrupción.
      if (src[i + k] == 0) return Err::CobsInvalid;
      if (out >= cap) return Err::LenOverflow;
      dst[out++] = src[i + k];
    }
    i += grp;
    if (code != 0xFF && i < n) {
      if (out >= cap) return Err::LenOverflow;
      dst[out++] = 0;
    }
  }
  *out_n = out;
  return Err::Ok;
}

// ---------------------------------------------------------------------------
// CRC32 — tabla generada una vez. En el ESP32 real esto es
// ~esp_rom_crc32_le(~0, ...) (Q-1); acá la implementación de referencia.
// ---------------------------------------------------------------------------

namespace {
struct CrcTable {
  uint32_t t[256];
  constexpr CrcTable() : t{} {
    for (unsigned n = 0; n < 256; n++) {
      uint32_t c = n;
      for (int k = 0; k < 8; k++) {
        c = (c & 1) ? (0xEDB88320u ^ (c >> 1)) : (c >> 1);
      }
      t[n] = c;
    }
  }
};
constexpr CrcTable kCrc;
}  // namespace

uint32_t crc32_update(uint32_t state, const uint8_t* data, size_t n) {
  for (size_t i = 0; i < n; i++) {
    state = kCrc.t[(state ^ data[i]) & 0xFF] ^ (state >> 8);
  }
  return state;
}

uint32_t crc32(const uint8_t* data, size_t n) {
  return crc32_update(0xFFFFFFFFu, data, n) ^ 0xFFFFFFFFu;
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

void fw_begin(FrameWriter& w, uint8_t* buf, size_t cap, uint16_t seq,
              uint64_t t_base_us, uint8_t flags) {
  w.buf = buf;
  w.cap = cap;
  w.len = 0;
  w.rec_count = 0;
  w.overflow = false;
  if (cap < kFrameHeaderLen + 4) {
    w.overflow = true;
    return;
  }
  buf[0] = static_cast<uint8_t>(kProtocolVersion | (flags << 4));
  put_u16(buf + 1, seq);
  put_u64(buf + 3, t_base_us);
  put_u16(buf + 11, 0);  // rec_count, se corrige en fw_end
  w.len = kFrameHeaderLen;
}

bool fw_add_record(FrameWriter& w, uint8_t type, uint16_t dt_us,
                   const uint8_t* payload, size_t n) {
  if (w.overflow || n > 255) return false;
  // PR-04: el CRC (4 bytes) tiene que entrar después
  if (w.len + kRecordHeaderLen + n + 4 > w.cap) {
    return false;
  }
  uint8_t* p = w.buf + w.len;
  p[0] = type;
  p[1] = static_cast<uint8_t>(n);
  put_u16(p + 2, dt_us);
  for (size_t i = 0; i < n; i++) p[4 + i] = payload[i];
  w.len += kRecordHeaderLen + n;
  w.rec_count++;
  return true;
}

size_t fw_end(FrameWriter& w) {
  if (w.overflow || w.len + 4 > w.cap) return 0;
  put_u16(w.buf + 11, w.rec_count);
  const uint32_t crc = crc32(w.buf, w.len);
  put_u32(w.buf + w.len, crc);
  w.len += 4;
  return w.len;
}

Err frame_parse_header(const uint8_t* pre_cobs, size_t n, FrameHeader* h) {
  if (n < kFrameHeaderLen + 4) return Err::Truncated;
  const uint32_t stored = get_u32(pre_cobs + n - 4);
  if (crc32(pre_cobs, n - 4) != stored) return Err::CrcMismatch;
  const uint8_t ver_flags = pre_cobs[0];
  if ((ver_flags & 0x0F) != kProtocolVersion) return Err::BadVersion;
  h->ver = ver_flags & 0x0F;
  h->flags = ver_flags >> 4;
  h->seq = get_u16(pre_cobs + 1);
  h->t_base_us = get_u64(pre_cobs + 3);
  h->rec_count = get_u16(pre_cobs + 11);
  return Err::Ok;
}

Err frame_records(const uint8_t* pre_cobs, size_t n, RecordView* recs,
                  size_t max, size_t* out_count) {
  *out_count = 0;
  FrameHeader h;
  const Err e = frame_parse_header(pre_cobs, n, &h);
  if (e != Err::Ok) return e;
  const uint8_t* p = pre_cobs + kFrameHeaderLen;
  const uint8_t* end = pre_cobs + n - 4;  // sin el CRC
  size_t count = 0;
  for (uint16_t i = 0; i < h.rec_count; i++) {
    if (end - p < static_cast<ptrdiff_t>(kRecordHeaderLen)) return Err::LenOverflow;
    RecordView v;
    v.type = p[0];
    v.len = p[1];
    v.dt_us = get_u16(p + 2);
    v.payload = p + 4;
    if (end - (p + 4) < static_cast<ptrdiff_t>(v.len)) return Err::LenOverflow;
    if (count < max) recs[count] = v;
    count++;
    p += kRecordHeaderLen + v.len;
  }
  // rec_count es autoridad: sobras después del último record son corrupción
  if (p != end) return Err::LenOverflow;
  *out_count = count;
  return Err::Ok;
}

}  // namespace mole
