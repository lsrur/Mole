// SPDX-License-Identifier: MIT
// Runner de tests del codec C++ contra protocol/codec_vectors.json (T-14).
// Lee EL MISMO archivo que los tests de Rust (C-6). Cobertura:
//   cobs / crc32 / frame  → completa (encode + decode + must_fail)
//   record                → estructura del header en todos; payload
//                           reconstruido en los tipos que el firmware emite
//   args                  → recorrido tipado con typedefs, profundidad, PR-20
//   catalog               → catalog_hash CAT-08
// Lo que queda fuera de alcance se reporta como "skip", nunca en silencio.

#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <string>
#include <vector>

#include "mole_codec.h"
#include "nlohmann/json.hpp"

using json = nlohmann::json;
using bytes = std::vector<uint8_t>;

static int g_checked = 0, g_skipped = 0;
static std::vector<std::string> g_fails;

static void fail(const std::string& id, const std::string& msg) {
  g_fails.push_back(id + ": " + msg);
}

static bytes unhex(const std::string& s) {
  bytes out;
  out.reserve(s.size() / 2);
  for (size_t i = 0; i + 1 < s.size(); i += 2) {
    out.push_back(static_cast<uint8_t>(std::stoul(s.substr(i, 2), nullptr, 16)));
  }
  return out;
}

static std::string hex(const bytes& b) {
  std::string s;
  char tmp[3];
  for (uint8_t x : b) {
    std::snprintf(tmp, sizeof tmp, "%02x", x);
    s += tmp;
  }
  return s;
}

// ---------------------------------------------------------------------------
// Reconstrucción de payloads (espejo del set de anchors.py)
// ---------------------------------------------------------------------------

static void put16(bytes& o, uint16_t v) {
  o.push_back(static_cast<uint8_t>(v));
  o.push_back(static_cast<uint8_t>(v >> 8));
}
static void put32(bytes& o, uint32_t v) {
  put16(o, static_cast<uint16_t>(v));
  put16(o, static_cast<uint16_t>(v >> 16));
}
static void put_str(bytes& o, const std::string& s) {
  o.push_back(static_cast<uint8_t>(s.size()));
  o.insert(o.end(), s.begin(), s.end());
}

static const std::map<std::string, uint8_t> kWireByName = {
    {"u8", 0x01},  {"i8", 0x02},  {"u16", 0x03}, {"i16", 0x04}, {"u32", 0x05},
    {"i32", 0x06}, {"u64", 0x07}, {"i64", 0x08}, {"f32", 0x09}, {"f64", 0x0A},
    {"bool", 0x0B}, {"sym", 0x0C}, {"str", 0x0D}, {"ptr", 0x0E}, {"struct", 0xF0},
};

// Empaqueta un arg desde su JSON (v o bits_hex). false si fuera de alcance.
static bool pack_arg(bytes& o, const json& a) {
  const std::string t = a.at("t");
  if (t == "f32" || t == "f64") {
    const bytes b = unhex(a.at("bits_hex"));
    o.insert(o.end(), b.begin(), b.end());
    return true;
  }
  if (t == "struct") return false;  // el layout exacto vive en el registro
  if (t == "bool") {
    o.push_back(a.at("v").get<bool>() ? 1 : 0);
    return true;
  }
  if (t == "str") {
    if (a.contains("bytes_hex")) {
      const bytes b = unhex(a.at("bytes_hex"));
      o.push_back(static_cast<uint8_t>(b.size()));
      o.insert(o.end(), b.begin(), b.end());
    } else {
      put_str(o, a.at("v").get<std::string>());
    }
    return true;
  }
  if (t == "sym") {
    put16(o, a.at("v").get<uint16_t>());
    return true;
  }
  const size_t size = mole::wire_fixed_size(kWireByName.at(t));
  const int64_t v = a.at("v").is_number_unsigned()
                        ? static_cast<int64_t>(a.at("v").get<uint64_t>())
                        : a.at("v").get<int64_t>();
  uint64_t u = static_cast<uint64_t>(v);
  for (size_t i = 0; i < size; i++) {
    o.push_back(static_cast<uint8_t>(u & 0xFF));
    u >>= 8;
  }
  return true;
}

// Reconstruye el payload de los tipos que el firmware va a emitir en F1.
static bool rebuild_payload(bytes& o, const json& rec) {
  const std::string t = rec.value("type", "");
  if (t == "REC_SPAN_END") {
    put16(o, rec.at("span_id"));
  } else if (t == "REC_SYM_DEF") {
    put16(o, rec.at("sym_id"));
    o.push_back(rec.at("kind").get<uint8_t>());
    put16(o, rec.at("parent"));
    put_str(o, rec.at("name"));
  } else if (t == "REC_FMT_DEF") {
    put16(o, rec.at("fmt_id"));
    put16(o, rec.at("file_sym"));
    put16(o, rec.at("line"));
    o.push_back(rec.at("argc").get<uint8_t>());
    for (const auto& at : rec.at("arg_types")) {
      if (at.is_object()) {
        o.push_back(0xF0);
        put16(o, at.at("struct"));
      } else {
        o.push_back(kWireByName.at(at.get<std::string>()));
      }
    }
    put_str(o, rec.at("fmt"));
  } else if (t == "REC_LOG_FMT") {
    o.push_back(rec.at("level").get<uint8_t>());
    o.push_back(rec.at("task_id").get<uint8_t>());
    o.push_back(rec.at("core").get<uint8_t>());
    put16(o, rec.at("tag_sym"));
    put16(o, rec.at("fmt_id"));
    for (const auto& a : rec.at("args")) {
      if (!pack_arg(o, a)) return false;
    }
  } else if (t == "REC_WATCH") {
    put16(o, rec.at("sym"));
    o.push_back(kWireByName.at(rec.at("value").at("t").get<std::string>()));
    if (!pack_arg(o, rec.at("value"))) return false;
  } else if (t == "REC_TYPE_DEF") {
    put16(o, rec.at("type_id"));
    put_str(o, rec.at("name"));
    o.push_back(rec.at("nfields").get<uint8_t>());
    for (const auto& f : rec.at("fields")) {
      put16(o, f.at("name_sym"));
      o.push_back(kWireByName.at(f.at("wire").get<std::string>()));
      o.push_back(f.at("flags").get<uint8_t>());
      put16(o, f.at("offset"));
      put16(o, f.at("size"));
      put16(o, f.at("ref_type"));
    }
  } else if (t == "REC_ENUM_DEF") {
    put16(o, rec.at("type_id"));
    put_str(o, rec.at("name"));
    const uint8_t base = kWireByName.at(rec.at("wire").get<std::string>());
    o.push_back(base);
    o.push_back(rec.at("nentries").get<uint8_t>());
    const size_t esize = mole::wire_fixed_size(base);
    for (const auto& e : rec.at("entries")) {
      uint64_t u = static_cast<uint64_t>(e.at("value").get<int64_t>());
      for (size_t i = 0; i < esize; i++) {
        o.push_back(static_cast<uint8_t>(u & 0xFF));
        u >>= 8;
      }
      put16(o, e.at("name_sym"));
    }
  } else if (t == "REC_STATE") {
    put16(o, rec.at("machine_sym"));
    put16(o, rec.at("state_sym"));
  } else if (t == "REC_STATUS") {
    put16(o, rec.at("sym"));
    o.push_back(rec.at("level").get<uint8_t>());
  } else if (t == "REC_EVENT") {
    put16(o, rec.at("sym"));
    put32(o, rec.at("arg"));
  } else if (t == "REC_SPAN_BEGIN") {
    put16(o, rec.at("span_id"));
    put16(o, rec.at("sym"));
    put16(o, rec.at("parent_span_id"));
    o.push_back(rec.at("task_id").get<uint8_t>());
  } else if (t == "REC_SPAN_ABORT") {
    put16(o, rec.at("span_id"));
    o.push_back(rec.at("reason").get<uint8_t>());
  } else if (t == "REC_PONG") {
    put32(o, rec.at("nonce"));
  } else if (t == "REC_PAUSED") {
    o.push_back(rec.at("task_id").get<uint8_t>());
    put16(o, rec.at("file_sym"));
    put16(o, rec.at("line"));
    o.push_back(rec.at("reason").get<uint8_t>());
  } else if (t == "REC_RESUMED") {
    o.push_back(rec.at("task_id").get<uint8_t>());
    o.push_back(rec.at("reason").get<uint8_t>());
  } else {
    return false;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Recorrido tipado de args (REC-45/REC-46) para los vectores de capa args
// ---------------------------------------------------------------------------

struct FieldD {
  uint8_t wire, flags;
  uint16_t size, ref_type;
};
using TypeMap = std::map<uint16_t, std::vector<FieldD>>;
using EnumMap = std::map<uint16_t, uint8_t>;  // type_id → wire base (draft.11)

// error: "" ok, o nombre must_fail
static std::string walk_value(uint8_t wire, uint16_t type_id, const bytes& b,
                              size_t& pos, size_t depth, const TypeMap& types,
                              const EnumMap& enums) {
  if (wire == mole::WIRE_STR) {
    if (pos >= b.size()) return "arg_count_mismatch";
    const size_t n = b[pos];
    pos += 1 + n;
    if (pos > b.size()) return "arg_count_mismatch";
    return "";
  }
  if (wire == mole::kArgTagEnum) {
    // enum suelto: el entero base sale de la definición (REC-53)
    const auto it = enums.find(type_id);
    if (it == enums.end()) return "unknown_fmt";
    pos += mole::wire_fixed_size(it->second);
    if (pos > b.size()) return "arg_count_mismatch";
    return "";
  }
  if (wire == mole::WIRE_STRUCT) {
    if (depth >= mole::kMaxStructDepth) return "depth_exceeded";
    const auto it = types.find(type_id);
    if (it == types.end()) return "unknown_fmt";
    for (const auto& f : it->second) {
      if (f.flags & mole::kFieldFlagArray) {
        // REC-54: count = size / tamaño(wire), solo escalares
        const size_t elem = mole::wire_fixed_size(f.wire);
        if (elem == 0 || f.size == 0 || f.size % elem != 0) return "arg_count_mismatch";
        pos += f.size / elem * elem;
        if (pos > b.size()) return "arg_count_mismatch";
        continue;
      }
      // un campo enum avanza por su entero base: mismo camino que un escalar
      const std::string e =
          walk_value(f.wire, f.ref_type, b, pos, depth + 1, types, enums);
      if (!e.empty()) return e;
    }
    return "";
  }
  const size_t size = mole::wire_fixed_size(wire);
  if (size == 0) return "arg_count_mismatch";
  pos += size;
  if (pos > b.size()) return "arg_count_mismatch";
  return "";
}

static std::string walk_args(const bytes& arg_types, const bytes& payload,
                             const TypeMap& types, const EnumMap& enums) {
  size_t pos = 0;
  size_t i = 0;
  while (i < arg_types.size()) {
    const uint8_t tag = arg_types[i++];
    uint16_t type_id = 0;
    if (tag == mole::WIRE_STRUCT || tag == mole::kArgTagEnum) {
      type_id = mole::get_u16(arg_types.data() + i);
      i += 2;
    }
    const std::string e = walk_value(tag, type_id, payload, pos, 0, types, enums);
    if (!e.empty()) return e;
  }
  if (pos != payload.size()) return "arg_count_mismatch";
  return "";
}

// Parsea un payload de REC_TYPE_DEF al TypeMap (para requires).
static void apply_typedef(TypeMap& types, const bytes& payload) {
  size_t p = 2;  // type_id
  const uint16_t type_id = mole::get_u16(payload.data());
  p += 1 + payload[p];  // name (PR-20)
  const uint8_t nfields = payload[p++];
  std::vector<FieldD> fields;
  for (uint8_t i = 0; i < nfields; i++) {
    FieldD f{};
    f.wire = payload[p + 2];
    f.flags = payload[p + 3];
    f.size = mole::get_u16(payload.data() + p + 6);
    f.ref_type = mole::get_u16(payload.data() + p + 8);
    fields.push_back(f);
    p += 10;
  }
  types[type_id] = fields;
}

// Parsea un payload de REC_ENUM_DEF al EnumMap (draft.11).
static void apply_enumdef(EnumMap& enums, const bytes& payload) {
  size_t p = 2;
  const uint16_t type_id = mole::get_u16(payload.data());
  p += 1 + payload[p];  // name
  enums[type_id] = payload[p];  // wire base
}

// ---------------------------------------------------------------------------

int main(int argc, char** argv) {
  const std::string path =
      argc > 1 ? argv[1] : "../../../protocol/codec_vectors.json";
  std::ifstream in(path);
  if (!in) {
    std::fprintf(stderr, "no pude abrir %s\n", path.c_str());
    return 2;
  }
  json doc = json::parse(in, nullptr, false);
  if (doc.is_discarded()) {
    std::fprintf(stderr, "JSON invalido\n");
    return 2;
  }

  std::map<std::string, json> by_id;
  for (const auto& v : doc["vectors"]) by_id[v["id"]] = v;

  for (const auto& v : doc["vectors"]) {
    const std::string id = v["id"];
    const std::string layer = v["layer"];
    const std::string mf = v.value("must_fail", "");

    if (layer == "cobs") {
      const bytes enc = unhex(v["encoded_hex"]);
      if (mf.empty()) {
        const bytes dec = unhex(v["decoded_hex"]);
        bytes out(mole::cobs_max_encoded(dec.size()));
        const size_t n = mole::cobs_encode(out.data(), out.size(), dec.data(), dec.size());
        out.resize(n);
        g_checked++;
        if (out != enc) fail(id, "encode COBS difiere");
        bytes back(enc.size() + 1);
        size_t bn = 0;
        if (mole::cobs_decode(back.data(), back.size(), &bn, enc.data(), enc.size()) != mole::Err::Ok ||
            bytes(back.begin(), back.begin() + bn) != dec) {
          fail(id, "decode COBS difiere");
        }
      } else {
        bytes back(enc.size() + 16);
        size_t bn = 0;
        const mole::Err e = mole::cobs_decode(back.data(), back.size(), &bn, enc.data(), enc.size());
        g_checked++;
        if (std::string(mole::err_name(e)) != mf) fail(id, "esperaba " + mf + ", obtuve " + mole::err_name(e));
      }
    } else if (layer == "crc32") {
      const bytes data = unhex(v["data_hex"]);
      char buf[9];
      std::snprintf(buf, sizeof buf, "%08x", mole::crc32(data.data(), data.size()));
      g_checked++;
      if (v["crc_hex"] != std::string(buf)) fail(id, "crc difiere");
    } else if (layer == "frame") {
      if (mf.empty()) {
        // reconstruir con FrameWriter desde los records referenciados
        const bytes pre = unhex(v["pre_cobs_hex"]);
        const bytes wire = unhex(v["wire_hex"]);
        uint8_t flags = 0;
        for (const auto& f : v["frame"]["flags"]) {
          const std::string s = f;
          flags |= (s == "CATALOG") ? 1 : (s == "DROPS") ? 2 : (s == "PAUSED") ? 4 : 0;
        }
        bytes buf(mole::kFrameMax);
        mole::FrameWriter w;
        mole::fw_begin(w, buf.data(), buf.size(), v["frame"]["seq"], v["frame"]["t_base_us"], flags);
        for (const auto& rid : v["frame"]["records"]) {
          const bytes rb = unhex(by_id.at(rid)["bytes_hex"]);
          mole::fw_add_record(w, rb[0], mole::get_u16(rb.data() + 2), rb.data() + 4, rb[1]);
        }
        const size_t n = mole::fw_end(w);
        g_checked++;
        if (bytes(buf.begin(), buf.begin() + n) != pre) fail(id, "pre_cobs difiere");
        bytes enc(mole::cobs_max_encoded(n) + 1);
        const size_t en = mole::cobs_encode(enc.data(), enc.size(), buf.data(), n);
        enc.resize(en);
        enc.push_back(0);
        if (enc != wire) fail(id, "wire difiere");
        // y parsear lo que armamos
        mole::FrameHeader h;
        std::vector<mole::RecordView> recs(1024);
        size_t count = 0;
        if (mole::frame_records(pre.data(), pre.size(), recs.data(), recs.size(), &count) != mole::Err::Ok ||
            count != v["frame"]["records"].size()) {
          fail(id, "parseo del frame difiere");
        }
        if (mole::frame_parse_header(pre.data(), pre.size(), &h) != mole::Err::Ok ||
            h.seq != v["frame"]["seq"] || h.t_base_us != v["frame"]["t_base_us"] || h.flags != flags) {
          fail(id, "header parseado difiere");
        }
      } else {
        const bytes wire = unhex(v["wire_hex"]);
        bytes pre(mole::kFrameMax);
        size_t pn = 0;
        mole::Err e = mole::cobs_decode(pre.data(), pre.size(), &pn, wire.data(), wire.size());
        if (e == mole::Err::Ok) {
          mole::FrameHeader h;
          e = mole::frame_parse_header(pre.data(), pn, &h);
          if (e == mole::Err::Ok) {
            std::vector<mole::RecordView> recs(1024);
            size_t count = 0;
            e = mole::frame_records(pre.data(), pn, recs.data(), recs.size(), &count);
          }
        }
        g_checked++;
        if (std::string(mole::err_name(e)) != mf) fail(id, "esperaba " + mf + ", obtuve " + mole::err_name(e));
      }
    } else if (layer == "record") {
      const bytes b = unhex(v["bytes_hex"]);
      g_checked++;
      if (b.size() < 4 || b.size() != 4u + b[1]) {
        fail(id, "header PR-07 inconsistente con bytes_hex");
        continue;
      }
      if (mf == "unknown_type") continue;  // estructura verificada; el resto es del host
      bytes payload;
      if (rebuild_payload(payload, v["record"])) {
        bytes full;
        full.push_back(b[0]);
        full.push_back(static_cast<uint8_t>(payload.size()));
        put16(full, v["record"].value("dt_us", 0));
        full.insert(full.end(), payload.begin(), payload.end());
        if (full != b) fail(id, "payload reconstruido difiere: " + hex(full));
      } else {
        g_skipped++;
      }
    } else if (layer == "args") {
      const bytes types_b = unhex(v["arg_types_hex"]);
      const bytes payload = unhex(v["bytes_hex"]);
      TypeMap types;
      EnumMap enums;
      if (v.contains("requires")) {
        for (const auto& rid : v["requires"]) {
          const bytes rb = unhex(by_id.at(rid)["bytes_hex"]);
          const bytes pl(rb.begin() + 4, rb.end());
          if (rb[0] == mole::REC_ENUM_DEF) {
            apply_enumdef(enums, pl);
          } else {
            apply_typedef(types, pl);
          }
        }
      }
      const std::string e = walk_args(types_b, payload, types, enums);
      g_checked++;
      if (e != mf) fail(id, "esperaba '" + mf + "', obtuve '" + e + "'");
    } else if (layer == "catalog") {
      if (!mf.empty()) {
        g_skipped++;  // unknown_fmt es semantica del host
        continue;
      }
      uint32_t state = 0xFFFFFFFFu;
      for (const auto& rid : v["records"]) {
        const bytes rb = unhex(by_id.at(rid)["bytes_hex"]);
        state = mole::crc32_update(state, rb.data() + 4, rb.size() - 4);
      }
      char buf[9];
      std::snprintf(buf, sizeof buf, "%08x", state ^ 0xFFFFFFFFu);
      g_checked++;
      if (v["expect"]["catalog_hash_hex"] != std::string(buf)) fail(id, "catalog_hash difiere");
    } else {
      g_skipped++;
    }
  }

  std::printf("codec_test: %d verificados, %d fuera de alcance, %zu fallos\n",
              g_checked, g_skipped, g_fails.size());
  for (const auto& f : g_fails) std::printf("  FALLO %s\n", f.c_str());
  return g_fails.empty() ? 0 : 1;
}
