#!/usr/bin/env python3
"""
anchors.py — el tercer camino independiente (mole-f0-plan.md, V-1/V-2).

Escrito leyendo specs/mole-spec.md (v2.0.0-draft.9, §6/§7 CONGELADAS),
sin mirar el codec de Rust (C-2). Deliberadamente ingenuo y verboso:
cada función es una transcripción directa de la regla de la spec que cita.

Uso:
    python3 anchors.py            # recalcula los 12 ancla y verifica contra
                                  # los valores esperados de protocol/README.md
    python3 anchors.py --json     # emite los ancla como vectores JSON
                                  # (fragmento para codec_vectors.json, T-12)

Si alguna vez un valor de acá difiere del codec de Rust o del de C++,
el árbitro es el cálculo paso a paso de protocol/README.md.
"""

import json
import struct
import sys
import zlib

# ---------------------------------------------------------------------------
# COBS — spec §6.1, PR-01. Delimitador 0x00, overhead peor caso 1/254.
#
# Convención fijada por el ancla cobs-254 (ver README):
#   - El ENCODER emite el grupo final vacío (code 0x01) cuando el dato
#     termina exactamente en un bloque lleno de 254 bytes (comportamiento de
#     la implementación de referencia de Cheshire & Baker).
#   - El DECODER acepta ambas variantes (con y sin grupo final vacío):
#     decodifican al mismo payload.
# ---------------------------------------------------------------------------

def cobs_encode(data: bytes) -> bytes:
    out = bytearray()
    code_idx = 0          # posición del byte de código del grupo abierto
    out.append(0x00)      # placeholder del primer código
    code = 1
    for b in data:
        if b != 0x00:
            out.append(b)
            code += 1
        if b == 0x00 or code == 0xFF:
            out[code_idx] = code
            code_idx = len(out)
            out.append(0x00)
            code = 1
    out[code_idx] = code
    return bytes(out)


class CobsError(ValueError):
    pass


def cobs_decode(enc: bytes) -> bytes:
    if len(enc) == 0:
        raise CobsError("cobs_underrun: vacio")
    out = bytearray()
    i = 0
    while i < len(enc):
        code = enc[i]
        if code == 0x00:
            # un 0x00 interno es ilegal: es el delimitador de frame
            raise CobsError("cobs_invalid: 0x00 interno")
        i += 1
        n = code - 1
        if i + n > len(enc):
            raise CobsError("cobs_underrun: grupo trunco")
        out += enc[i : i + n]
        i += n
        if code != 0xFF and i < len(enc):
            out.append(0x00)
    return bytes(out)


# ---------------------------------------------------------------------------
# CRC32 — spec PR-03: "CRC32 (poly zlib)" sobre los bytes del frame.
#
# Convención fijada por el ancla crc32-canonical (riesgo Q-1 del plan):
#   crc = ~esp_rom_crc32_le(~0, buf, len)
# que es exactamente el CRC-32/ISO-HDLC estándar (poly reflejado 0xEDB88320,
# init 0xFFFFFFFF, xor final 0xFFFFFFFF) — el mismo que zlib.crc32.
# Valor de chequeo universal: CRC32("123456789") = 0xCBF43926.
#
# Se implementa bit a bit acá y se contrasta contra zlib como doble control.
# ---------------------------------------------------------------------------

def crc32_spec(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            if crc & 1:
                crc = (crc >> 1) ^ 0xEDB88320
            else:
                crc >>= 1
    return crc ^ 0xFFFFFFFF


def crc32_checked(data: bytes) -> int:
    a = crc32_spec(data)
    b = zlib.crc32(data) & 0xFFFFFFFF
    assert a == b, f"crc32 bit a bit ({a:08x}) difiere de zlib ({b:08x})"
    return a


# ---------------------------------------------------------------------------
# Primitivas de serialización. Todo little-endian (spec §6.2).
# ---------------------------------------------------------------------------

def u8(v):  return struct.pack("<B", v)
def u16(v): return struct.pack("<H", v)
def u32(v): return struct.pack("<I", v)
def u64(v): return struct.pack("<Q", v)
def f32(v): return struct.pack("<f", v)


def s(text: str) -> bytes:
    """Cadena con prefijo de longitud u8 — spec PR-20."""
    raw = text.encode("utf-8")
    assert len(raw) <= 255, "PR-20: prefijo u8"
    return u8(len(raw)) + raw


# Enum de tipos de wire — spec REC-52.
WIRE_U8, WIRE_I8, WIRE_U16, WIRE_I16 = 0x01, 0x02, 0x03, 0x04
WIRE_U32, WIRE_I32, WIRE_U64, WIRE_I64 = 0x05, 0x06, 0x07, 0x08
WIRE_F32, WIRE_F64, WIRE_BOOL = 0x09, 0x0A, 0x0B
WIRE_SYM, WIRE_STR, WIRE_PTR, WIRE_STRUCT = 0x0C, 0x0D, 0x0E, 0xF0

# SymKind — spec CAT-03.
KIND_WATCH, KIND_TAG, KIND_TASK, KIND_FILE = 1, 2, 3, 4
KIND_COMMAND, KIND_SPAN, KIND_COUNTER, KIND_MACHINE = 5, 6, 7, 8
KIND_STATE, KIND_BIND, KIND_DUMP, KIND_SCHEMA = 9, 10, 11, 12

# Tipos de record — spec §7.1.
REC_SYM_DEF, REC_FMT_DEF, REC_TYPE_DEF = 0x02, 0x05, 0x06
REC_LOG_FMT, REC_WATCH, REC_SPAN_END = 0x11, 0x20, 0x51


def record(rtype: int, dt_us: int, payload: bytes) -> bytes:
    """Header de record — spec PR-07: type u8, len u8, dt_us u16."""
    assert 0 <= len(payload) <= 255, "PR-07: len es u8"
    assert 0 <= dt_us <= 0xFFFF, "PR-08: dt_us es u16"
    return u8(rtype) + u8(len(payload)) + u16(dt_us) + payload


def frame(seq: int, t_base_us: int, records: list, flags: int = 0) -> tuple:
    """Frame — spec PR-02. Devuelve (pre_cobs, wire).

    ver_flags: bits 0-3 = versión (2), bits 4-7 = flags.
    CRC32 sobre bytes [0 .. len-5], almacenado little-endian al final.
    wire = COBS(pre_cobs) + 0x00 (delimitador, §6.1).
    """
    ver_flags = 2 | (flags << 4)
    body = u8(ver_flags) + u16(seq) + u64(t_base_us) + u16(len(records))
    for r in records:
        body += r
    pre_cobs = body + u32(crc32_checked(body))
    wire = cobs_encode(pre_cobs) + b"\x00"
    return pre_cobs, wire


# ---------------------------------------------------------------------------
# Los 12 vectores ancla (V-3). El cálculo paso a paso está en README.md;
# los valores esperados de acá abajo son la transcripción de ese documento.
# ---------------------------------------------------------------------------

def build_anchors():
    anchors = []

    # ---- capa cobs ----
    a1_dec = b""
    anchors.append({
        "id": "cobs-empty", "layer": "cobs", "anchor": True,
        "desc": "payload vacio: un solo byte de codigo 0x01",
        "decoded_hex": a1_dec.hex(), "encoded_hex": cobs_encode(a1_dec).hex(),
        "_expect_encoded": "01",
    })

    a2_dec = bytes.fromhex("11002233")
    anchors.append({
        "id": "cobs-zero-mid", "layer": "cobs", "anchor": True,
        "desc": "cero intercalado: bloques [11] y [22 33]",
        "decoded_hex": a2_dec.hex(), "encoded_hex": cobs_encode(a2_dec).hex(),
        "_expect_encoded": "0211032233",
    })

    a3_dec = b"\x41" * 254
    anchors.append({
        "id": "cobs-254", "layer": "cobs", "anchor": True,
        "desc": "bloque lleno de 254 no-ceros: code 0xFF + grupo final vacio 0x01 (convencion fijada)",
        "decoded_hex": a3_dec.hex(), "encoded_hex": cobs_encode(a3_dec).hex(),
        "_expect_encoded": "ff" + "41" * 254 + "01",
    })

    # ---- capa crc32 ----
    a4_data = b"123456789"
    anchors.append({
        "id": "crc32-canonical", "layer": "crc32", "anchor": True,
        "desc": "valor de chequeo universal de CRC-32/ISO-HDLC; fija la convencion Q-1",
        "data_hex": a4_data.hex(), "crc_hex": f"{crc32_checked(a4_data):08x}",
        "_expect_crc": "cbf43926",
    })

    # ---- capa record ----
    a5 = record(REC_SPAN_END, 250, u16(7))
    anchors.append({
        "id": "rec-span-end", "layer": "record", "anchor": True,
        "desc": "record minimo: header PR-07 + span_id",
        "record": {"type": "REC_SPAN_END", "dt_us": 250, "span_id": 7},
        "bytes_hex": a5.hex(),
        "_expect_bytes": "5102fa000700",
    })

    a6_payload = u16(1) + u8(KIND_TAG) + u16(0) + s("net")
    a6 = record(REC_SYM_DEF, 0, a6_payload)
    anchors.append({
        "id": "rec-sym-def", "layer": "record", "anchor": True,
        "desc": "catalogo CAT-04 con cadena PR-20: sym 1, kind Tag(2), sin parent, nombre 'net'",
        "record": {"type": "REC_SYM_DEF", "dt_us": 0, "sym_id": 1, "kind": 2,
                   "parent": 0, "name": "net"},
        "bytes_hex": a6.hex(),
        "_expect_bytes": "020900000100020000036e6574",
    })

    a7_fmt = "crudo={} v={:.2}"
    a7_payload = (u16(7) + u16(4) + u16(42) + u8(2)
                  + u8(WIRE_I32) + u8(WIRE_F32) + s(a7_fmt))
    a7 = record(REC_FMT_DEF, 0, a7_payload)
    anchors.append({
        "id": "rec-fmt-def", "layer": "record", "anchor": True,
        "desc": "REC-34: fmt_id 7, origen sym 4 linea 42, argc 2 (i32, f32), fmt con llaves",
        "record": {"type": "REC_FMT_DEF", "dt_us": 0, "fmt_id": 7, "file_sym": 4,
                   "line": 42, "argc": 2, "arg_types": ["i32", "f32"], "fmt": a7_fmt},
        "bytes_hex": a7.hex(),
        "_expect_bytes": "051a0000070004002a00020609"
                         + "10637275646f3d7b7d20763d7b3a2e327d",
    })

    a8_payload = (u8(2) + u8(3) + u8(1) + u16(1) + u16(7)
                  + struct.pack("<i", 1024) + f32(2.47))
    a8 = record(REC_LOG_FMT, 1500, a8_payload)
    anchors.append({
        "id": "rec-log-fmt", "layer": "record", "anchor": True,
        "desc": "REC-35: INFO, tarea 3 core 1, tag 1, fmt 7, args (i32 1024, f32 2.47) sin etiquetas de tipo",
        "record": {"type": "REC_LOG_FMT", "dt_us": 1500, "level": 2, "task_id": 3,
                   "core": 1, "tag_sym": 1, "fmt_id": 7,
                   "args": [{"t": "i32", "v": 1024},
                            {"t": "f32", "v": 2.47, "bits_hex": "7b141e40"}]},
        "bytes_hex": a8.hex(),
        "_expect_bytes": "110fdc050203010100070000040000" + "7b141e40",
    })

    a9 = record(REC_WATCH, 10, u16(5) + u8(WIRE_F32) + f32(1.5))
    anchors.append({
        "id": "rec-watch", "layer": "record", "anchor": True,
        "desc": "REC-06: watch sym 5, f32 1.5",
        "record": {"type": "REC_WATCH", "dt_us": 10, "sym": 5,
                   "value": {"t": "f32", "v": 1.5, "bits_hex": "0000c03f"}},
        "bytes_hex": a9.hex(),
        "_expect_bytes": "20070a000500090000c03f",
    })

    # struct Sens { uint8_t a; uint16_t b; } con b big-endian (registro de sensor).
    # Layout del compilador: a en offset 0, 1 byte de padding, b en offset 2, sizeof 4.
    a10_field_a = u16(2) + u8(WIRE_U8) + u8(0x00) + u16(0) + u16(1) + u16(0)
    a10_field_b = u16(3) + u8(WIRE_U16) + u8(0x01) + u16(2) + u16(2) + u16(0)
    a10_payload = u16(1) + s("Sens") + u8(2) + a10_field_a + a10_field_b
    a10 = record(REC_TYPE_DEF, 0, a10_payload)
    anchors.append({
        "id": "rec-type-def", "layer": "record", "anchor": True,
        "desc": "REC-43 draft.9: descriptor con flags por campo; el campo b lleva bit 0 (big-endian)",
        "record": {"type": "REC_TYPE_DEF", "dt_us": 0, "type_id": 1, "name": "Sens",
                   "nfields": 2,
                   "fields": [
                       {"name_sym": 2, "wire": "u8", "flags": 0, "offset": 0, "size": 1, "ref_type": 0},
                       {"name_sym": 3, "wire": "u16", "flags": 1, "offset": 2, "size": 2, "ref_type": 0}]},
        "bytes_hex": a10.hex(),
        "_expect_bytes": "061c00000100" + "0453656e73" + "02"
                         + "02000100000001000000"
                         + "03000301020002000000",
    })

    # ---- capa args ----
    # Memoria del Sens de arriba: a=0x11, padding=0x00, b=0x2233 guardado
    # big-endian (tal como lo dejo el sensor). Modo fiel a los valores
    # (REC-45): secuencial, sin padding, bytes tal como estan en memoria
    # (el host invierte los campos BE al decodificar, no el MCU).
    anchors.append({
        "id": "args-struct-values", "layer": "args", "anchor": True,
        "desc": "REC-45 modo valores: Sens{a=0x11, b=BE 0x2233}; el padding no viaja, el byteswap lo hace el host",
        "args": [{"t": "struct", "type_id": 1,
                  "v": {"a": 17, "b": 8755, "_memoria_hex": "11002233"}}],
        "bytes_hex": "112233",
        "_expect_bytes": "112233",
    })

    # ---- capa frame ----
    a12_pre, a12_wire = frame(seq=1, t_base_us=1_000_000, records=[a5, a9])
    anchors.append({
        "id": "frame-two-records", "layer": "frame", "anchor": True,
        "desc": "PR-02 completo: ver 2, seq 1, t_base 1e6, span-end + watch, CRC32 LE, COBS + 0x00",
        "frame": {"ver": 2, "flags": [], "seq": 1, "t_base_us": 1_000_000,
                  "records": ["rec-span-end", "rec-watch"]},
        "pre_cobs_hex": a12_pre.hex(), "wire_hex": a12_wire.hex(),
        "_expect_pre_cobs": "02010040420f000000000002005102fa000700"
                            "20070a000500090000c03f53257235",
        "_expect_wire": "0302010440420f010101010202045102fa0207"
                        "0420070a020502090107c03f5325723500",
    })

    return anchors


def self_check(anchors) -> int:
    failures = 0
    for a in anchors:
        checks = []
        if "_expect_encoded" in a:
            checks.append(("encoded_hex", a["_expect_encoded"]))
        if "_expect_crc" in a:
            checks.append(("crc_hex", a["_expect_crc"]))
        if "_expect_bytes" in a:
            checks.append(("bytes_hex", a["_expect_bytes"]))
        if a.get("_expect_pre_cobs") is not None:
            checks.append(("pre_cobs_hex", a["_expect_pre_cobs"]))
        if a.get("_expect_wire") is not None:
            checks.append(("wire_hex", a["_expect_wire"]))
        for field, expected in checks:
            got = a[field]
            status = "ok" if got == expected else "FALLO"
            if got != expected:
                failures += 1
                print(f"[{status}] {a['id']}.{field}\n  esperado: {expected}\n  calculado: {got}")
            else:
                print(f"[{status}] {a['id']}.{field}")
        if not checks:
            print(f"[calc] {a['id']}: sin valor esperado embebido todavia")
            for k in ("pre_cobs_hex", "wire_hex"):
                if k in a:
                    print(f"  {k} = {a[k]}")

    # ida y vuelta de COBS sobre todos los ancla que lo usan
    for a in anchors:
        if a["layer"] == "cobs":
            rt = cobs_decode(bytes.fromhex(a["encoded_hex"]))
            assert rt.hex() == a["decoded_hex"], f"round-trip COBS fallo en {a['id']}"
        if a["layer"] == "frame":
            wire = bytes.fromhex(a["wire_hex"])
            assert wire[-1] == 0x00, "el wire debe terminar en el delimitador"
            rt = cobs_decode(wire[:-1])
            assert rt.hex() == a["pre_cobs_hex"], f"round-trip COBS fallo en {a['id']}"
    print("round-trip COBS: ok")
    return failures


# ---------------------------------------------------------------------------
# Verificación cruzada (V-2.3): recalcula los vectores de codec_vectors.json
# que están dentro del alcance de este script y falla si difieren.
# ---------------------------------------------------------------------------

WIRE_BY_NAME = {
    "u8": 0x01, "i8": 0x02, "u16": 0x03, "i16": 0x04, "u32": 0x05, "i32": 0x06,
    "u64": 0x07, "i64": 0x08, "f32": 0x09, "f64": 0x0A, "bool": 0x0B,
    "sym": 0x0C, "str": 0x0D, "ptr": 0x0E, "struct": 0xF0,
}
FLAG_BY_NAME = {"CATALOG": 0x1, "DROPS": 0x2, "PAUSED": 0x4}
REC_OPCODES = {
    "REC_SPAN_END": 0x51, "REC_SYM_DEF": 0x02, "REC_FMT_DEF": 0x05,
    "REC_LOG_FMT": 0x11, "REC_WATCH": 0x20, "REC_TYPE_DEF": 0x06,
    "REC_ENUM_DEF": 0x07, "REC_STATE": 0x70, "REC_STATUS": 0x71,
    "REC_EVENT": 0x81, "REC_SPAN_BEGIN": 0x50, "REC_SPAN_ABORT": 0x52,
    "REC_PONG": 0x04, "REC_PAUSED": 0xF0, "REC_RESUMED": 0xF1,
}
WIRE_SIZES = {"u8": 1, "i8": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4,
              "u64": 8, "i64": 8, "f32": 4, "f64": 8, "bool": 1}


def rebuild_arg_bytes(args_json):
    """Empaqueta args desde su representación JSON (v o bits_hex)."""
    out = b""
    for a in args_json:
        t = a["t"]
        if t in ("f32", "f64"):
            out += bytes.fromhex(a["bits_hex"])
        elif t == "struct":
            return None  # los campos exactos viven en el registro de tipos: fuera de alcance
        elif t == "bool":
            out += u8(1 if a["v"] else 0)
        elif t == "str":
            raw = bytes.fromhex(a["bytes_hex"]) if "bytes_hex" in a else a["v"].encode()
            out += s(raw.decode("latin-1"))
        elif t == "sym":
            out += u16(a["v"])
        else:
            size = {"u8": 1, "i8": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4,
                    "u64": 8, "i64": 8, "ptr": 4}[t]
            out += int(a["v"]).to_bytes(size, "little", signed=a["v"] < 0)
    return out


def rebuild_record(rec):
    """Reconstruye el payload de los tipos de record al alcance del script."""
    t = rec.get("type")
    if t == "REC_SPAN_END":
        return u16(rec["span_id"])
    if t == "REC_SYM_DEF":
        return u16(rec["sym_id"]) + u8(rec["kind"]) + u16(rec["parent"]) + s(rec["name"])
    if t == "REC_FMT_DEF":
        types_b = b""
        for at in rec["arg_types"]:
            if isinstance(at, dict):
                types_b += u8(0xF0) + u16(at["struct"])
            else:
                types_b += u8(WIRE_BY_NAME[at])
        return (u16(rec["fmt_id"]) + u16(rec["file_sym"]) + u16(rec["line"])
                + u8(rec["argc"]) + types_b + s(rec["fmt"]))
    if t == "REC_LOG_FMT":
        args_b = rebuild_arg_bytes(rec["args"])
        if args_b is None:
            return None
        return (u8(rec["level"]) + u8(rec["task_id"]) + u8(rec["core"])
                + u16(rec["tag_sym"]) + u16(rec["fmt_id"]) + args_b)
    if t == "REC_WATCH":
        val_b = rebuild_arg_bytes([rec["value"]])
        if val_b is None:
            return None
        return u16(rec["sym"]) + u8(WIRE_BY_NAME[rec["value"]["t"]]) + val_b
    if t == "REC_TYPE_DEF":
        body = u16(rec["type_id"]) + s(rec["name"]) + u8(rec["nfields"])
        for f in rec["fields"]:
            body += (u16(f["name_sym"]) + u8(WIRE_BY_NAME[f["wire"]]) + u8(f["flags"])
                     + u16(f["offset"]) + u16(f["size"]) + u16(f["ref_type"]))
        return body
    if t == "REC_ENUM_DEF":
        body = u16(rec["type_id"]) + s(rec["name"]) + u8(WIRE_BY_NAME[rec["wire"]])
        body += u8(rec["nentries"])
        size = WIRE_SIZES[rec["wire"]]
        for e in rec["entries"]:
            body += int(e["value"]).to_bytes(size, "little", signed=e["value"] < 0)
            body += u16(e["name_sym"])
        return body
    if t == "REC_STATE":
        return u16(rec["machine_sym"]) + u16(rec["state_sym"])
    if t == "REC_STATUS":
        return u16(rec["sym"]) + u8(rec["level"])
    if t == "REC_EVENT":
        return u16(rec["sym"]) + u32(rec["arg"])
    if t == "REC_SPAN_BEGIN":
        return (u16(rec["span_id"]) + u16(rec["sym"]) + u16(rec["parent_span_id"])
                + u8(rec["task_id"]))
    if t == "REC_SPAN_ABORT":
        return u16(rec["span_id"]) + u8(rec["reason"])
    if t == "REC_PONG":
        return u32(rec["nonce"])
    if t == "REC_PAUSED":
        return u8(rec["task_id"]) + u16(rec["file_sym"]) + u16(rec["line"]) + u8(rec["reason"])
    if t == "REC_RESUMED":
        return u8(rec["task_id"]) + u8(rec["reason"])
    return None


def check_file(path):
    with open(path) as fh:
        doc = json.load(fh)
    vectors = {v["id"]: v for v in doc["vectors"]}
    checked, skipped, failures = 0, 0, []

    def mismatch(vid, what, expected, got):
        failures.append(f"{vid}.{what}\n  archivo:    {expected}\n  recalculado: {got}")

    for v in doc["vectors"]:
        vid, layer = v["id"], v["layer"]
        if layer == "cobs" and "must_fail" not in v:
            enc = cobs_encode(bytes.fromhex(v["decoded_hex"])).hex()
            checked += 1
            if enc != v["encoded_hex"]:
                mismatch(vid, "encoded_hex", v["encoded_hex"], enc)
        elif layer == "cobs":
            checked += 1
            try:
                cobs_decode(bytes.fromhex(v["encoded_hex"]))
                failures.append(f"{vid}: debia fallar ({v['must_fail']}) y decodifico")
            except CobsError:
                pass
        elif layer == "crc32":
            crc = f"{crc32_checked(bytes.fromhex(v['data_hex'])):08x}"
            checked += 1
            if crc != v["crc_hex"]:
                mismatch(vid, "crc_hex", v["crc_hex"], crc)
        elif layer == "record" and "record" in v:
            payload = rebuild_record(v["record"])
            if payload is None:
                skipped += 1
                continue
            rec_bytes = record(
                REC_OPCODES[v["record"]["type"]], v["record"].get("dt_us", 0), payload
            ).hex()
            checked += 1
            if rec_bytes != v["bytes_hex"]:
                mismatch(vid, "bytes_hex", v["bytes_hex"], rec_bytes)
        elif layer == "frame" and "must_fail" not in v:
            f = v["frame"]
            recs = [bytes.fromhex(vectors[rid]["bytes_hex"]) for rid in f["records"]]
            flags = 0
            for name in f["flags"]:
                flags |= FLAG_BY_NAME[name]
            pre, wire = frame(f["seq"], f["t_base_us"], recs, flags)
            checked += 1
            if pre.hex() != v["pre_cobs_hex"]:
                mismatch(vid, "pre_cobs_hex", v["pre_cobs_hex"], pre.hex())
            if wire.hex() != v["wire_hex"]:
                mismatch(vid, "wire_hex", v["wire_hex"], wire.hex())
        elif layer == "catalog" and "must_fail" not in v:
            # CAT-08: CRC32 incremental sobre payloads de definiciones en orden
            blob = b""
            for rid in v["records"]:
                rec_bytes = bytes.fromhex(vectors[rid]["bytes_hex"])
                blob += rec_bytes[4:]
            got = f"{crc32_checked(blob):08x}"
            expected = v["expect"]["catalog_hash_hex"]
            checked += 1
            if got != expected:
                mismatch(vid, "catalog_hash", expected, got)
        else:
            skipped += 1

    print(f"verificacion cruzada: {checked} recalculados, {skipped} fuera de alcance")
    if failures:
        print("\n".join(failures))
        return 1
    return 0


def main():
    anchors = build_anchors()
    if "--check" in sys.argv:
        idx = sys.argv.index("--check")
        path = sys.argv[idx + 1] if len(sys.argv) > idx + 1 else "codec_vectors.json"
        return check_file(path)
    if "--json" in sys.argv:
        clean = [{k: v for k, v in a.items() if not k.startswith("_")} for a in anchors]
        json.dump(clean, sys.stdout, indent=2, ensure_ascii=False)
        print()
        return 0
    failures = self_check(anchors)
    if failures:
        print(f"\n{failures} discrepancia(s). El arbitro es protocol/README.md (V-3).")
        return 1
    print(f"\n{len(anchors)} ancla verificados.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
