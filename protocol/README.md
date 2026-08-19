# protocol/

Artefactos compartidos del protocolo Mole v2 (spec §6/§7, **congeladas en draft.9**).

| Archivo | Qué es |
|---|---|
| `codec_vectors.json` | Set de vectores compartido entre Rust y C++ (se genera en T-12) |
| `codec_vectors.schema.json` | JSON Schema del formato de vectores (T-02) |
| `anchors.py` | Tercer camino independiente: recalcula los ancla leyendo la spec, no el código (T-03) |

`codec_vectors.json` se commitea y CI lo valida; **no se regenera en CI** (V-7). Si un cambio de código lo modifica, eso aparece en el diff del PR y se discute.

## Por qué existe `anchors.py`

Si los vectores los genera el codec de Rust y el de C++ se valida contra ellos, un error en Rust se convierte en el estándar (V-1). `anchors.py` se escribió **leyendo la spec, sin mirar el código de Rust** (C-2). Los 12 vectores ancla de abajo están calculados a mano acá; si alguna vez una implementación difiere, **este documento es el árbitro** (V-3).

```
python3 anchors.py          # recalcula y verifica los 12 ancla
python3 anchors.py --json   # los emite como vectores JSON
```

## Convenciones fijadas (las respuestas a Q-1 y compañía)

**CRC32 (Q-1).** `esp_rom_crc32_le()` opera con semántica invertida; el uso correcto es `~esp_rom_crc32_le(~0, buf, len)`, que equivale al **CRC-32/ISO-HDLC estándar**: polinomio reflejado `0xEDB88320`, init `0xFFFFFFFF`, XOR final `0xFFFFFFFF` — el mismo algoritmo que `zlib.crc32`. Valor de chequeo universal: `CRC32("123456789") = 0xCBF43926` (ancla `crc32-canonical`). En los vectores, `crc_hex` es **el valor u32 en hex**; dentro del frame se almacena **little-endian** (`0x35722553` → bytes `53 25 72 35`).

**COBS en el borde de 254 (ancla `cobs-254`).** Cuando el payload termina exactamente al llenarse un bloque de 254 no-ceros, existen dos codificaciones válidas que decodifican igual (con y sin grupo final vacío). Convención fijada: **el encoder emite el grupo final vacío** (código `0x01`), como la implementación de referencia de Cheshire & Baker. El decoder DEBE aceptar ambas.

**Delimitador.** En vectores de capa `frame`, `wire_hex` **incluye** el `0x00` final. En capa `cobs`, `encoded_hex` es solo la salida COBS, sin delimitador.

**Cadenas (PR-20).** Todo `char[]` lleva prefijo de longitud `u8`, incluso como último campo.

**Byteswap (Q-2, resuelto en spec draft.8/9).** El MCU nunca invierte: los campos marcados big-endian (`flags` bit 0 en `REC_TYPE_DEF`) viajan tal como están en memoria y el host los invierte al decodificar.

**Modo valores (Q-3).** El empaquetado fiel a los valores es secuencial y sin padding: el `offset` del descriptor **no sirve** para decodificarlo (es del modo bytes); el host recorre los campos en orden acumulando tamaños (ancla `args-struct-values`).

## Los 12 vectores ancla

Referencias: PR-07 (header de record: `type u8, len u8, dt_us u16`), REC-52 (enum de tipos: `i32=0x06`, `f32=0x09`, `u8=0x01`, `u16=0x03`), CAT-03 (`Tag=2`). Todo little-endian salvo indicación.

### 1. `cobs-empty`
Payload vacío ⇒ un único grupo vacío: código `0x01`.
`"" → 01`

### 2. `cobs-zero-mid`
`11 00 22 33` ⇒ bloques `[11]` y `[22 33]` ⇒ `(02) 11 (03) 22 33`.
`11002233 → 0211032233`

### 3. `cobs-254`
254 bytes `0x41` ⇒ bloque lleno: `(FF)` + 254×`41` + grupo final vacío `(01)` (convención de arriba).

### 4. `crc32-canonical`
`"123456789"` = `313233343536373839` ⇒ `crc = cbf43926`. Verificado bit a bit y contra zlib.

### 5. `rec-span-end`
`REC_SPAN_END(0x51)`, `dt_us=250 (fa00)`, payload `span_id=7 (0700)`, len 2.
`51 02 fa00 | 0700` → `5102fa000700`

### 6. `rec-sym-def`
`REC_SYM_DEF(0x02)`, dt 0. Payload CAT-04: `sym_id=1 (0100)`, `kind=Tag (02)`, `parent=0 (0000)`, `name="net"` con PR-20 (`03 6e6574`) ⇒ 9 bytes.
`02 09 0000 | 0100 02 0000 03 6e6574` → `020900000100020000036e6574`

### 7. `rec-fmt-def`
`REC_FMT_DEF(0x05)`, dt 0. Payload REC-34: `fmt_id=7 (0700)`, `file_sym=4 (0400)`, `line=42 (2a00)`, `argc=2 (02)`, `arg_types=[i32,f32] (06 09)`, `fmt="crudo={} v={:.2}"` (16 chars: `10` + ASCII) ⇒ 26 bytes = `0x1a`.
`05 1a 0000 | 0700 0400 2a00 02 0609 10 637275646f3d7b7d20763d7b3a2e327d`

### 8. `rec-log-fmt`
`REC_LOG_FMT(0x11)`, `dt_us=1500 (dc05)`. Payload REC-35: `level=INFO (02)`, `task=3 (03)`, `core=1 (01)`, `tag_sym=1 (0100)`, `fmt_id=7 (0700)`, args **sin etiquetas de tipo** (los tipos viajaron en el FMT_DEF): `i32 1024 (00040000)`, `f32 2.47 (bits 401e147b → LE 7b141e40)` ⇒ 15 bytes = `0x0f`.
`11 0f dc05 | 02 03 01 0100 0700 00040000 7b141e40`

### 9. `rec-watch`
`REC_WATCH(0x20)`, `dt_us=10 (0a00)`. Payload REC-06: `sym=5 (0500)`, `type=f32 (09)`, `1.5f (bits 3fc00000 → LE 0000c03f)` ⇒ **7 bytes**.
`20 07 0a00 | 0500 09 0000c03f`
*Nota histórica: el primer cálculo a mano decía `len=8`; el self-check de `anchors.py` lo detectó. Quede como evidencia de por qué existe el tercer camino.*

### 10. `rec-type-def`
`struct Sens { uint8_t a; uint16_t b; }` con `b` big-endian. Layout del compilador: `a` en offset 0 (1 byte), 1 byte de padding, `b` en offset 2, `sizeof=4`.
`REC_TYPE_DEF(0x06)`, dt 0. Payload REC-43 (draft.9, con `flags`): `type_id=1 (0100)`, `name="Sens" (04 53656e73)`, `nfields=2 (02)`, campos de 10 bytes `{name_sym u16, wire u8, flags u8, offset u16, size u16, ref_type u16}`:
- `a`: `0200 01 00 0000 0100 0000` (wire u8, flags 0)
- `b`: `0300 03 01 0200 0200 0000` (wire u16, **flags bit 0 = BE**)

⇒ 28 bytes = `0x1c`: `06 1c 0000 | 0100 0453656e73 02 02000100000001000000 03000301020002000000`

### 11. `args-struct-values`
Memoria del `Sens` anterior con `a=0x11`, padding `0x00`, `b=0x2233` almacenado BE: `11 00 22 33`. Modo fiel a los valores (REC-45): secuencial, **sin padding**, bytes tal como están en memoria (el swap de `b` lo hace el host).
`11002233 (memoria) → 112233 (wire)`

### 12. `frame-two-records`
Frame PR-02: `ver_flags=02` (ver 2, sin flags), `seq=1 (0100)`, `t_base_us=1.000.000 (40420f0000000000)`, `rec_count=2 (0200)`, records №5 y №9 concatenados. CRC32 sobre los 30 bytes previos = `0x35722553`, almacenado LE (`53257235`).

```
pre_cobs = 02 0100 40420f0000000000 0200 5102fa000700 20070a000500090000c03f 53257235
wire     = COBS(pre_cobs) + 00
         = 0302010440420f010101010202045102fa020704
           20070a020502090107c03f5325723500
```

## Cobertura de los ancla

| Primitiva | Ancla |
|---|---|
| COBS (vacío, cero interno, borde 254) | 1, 2, 3 |
| CRC32 y su convención | 4, 12 |
| Header de record PR-07 | 5–10 |
| Cadenas PR-20 (interior y final) | 6, 7, 10 |
| `arg_types` y empaquetado de args REC-35 | 7, 8 |
| Enum de tipos REC-52 | 7, 8, 9, 10 |
| Descriptor de struct con `flags` BE (draft.9) | 10 |
| Modo valores sin padding (Q-3) | 11 |
| Frame completo + CRC + COBS + delimitador | 12 |

El resto de la matriz de §6 del plan se cubre con vectores derivados en T-12; `anchors.py` recalcula los que estén dentro de su alcance (V-2.3).
