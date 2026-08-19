---
doc: mole-f0-plan.md
version: 1.0.1
fecha: 2026-08-18
estado: PARA EJECUTAR
depende_de: mole-spec.md v2.0.0-draft.9 (§6 y §7 congeladas)
ejecutor: Claude Code
---

# Mole v2 — Plan de F0: codec y vectores compartidos

## Changelog

| Versión | Fecha | Cambios |
|---|---|---|
| 1.0.0 | 2026-08-18 | Plan inicial de F0. |
| 1.0.1 | 2026-08-18 | Dependencia actualizada a spec draft.8. Q-2 resuelto por la spec: flag por campo en `REC_TYPE_DEF`, el host invierte. |
| 1.0.2 | 2026-08-18 | R-1/T-01: v1 se elimina por completo — sin tag `v1-archive` ni rama `v2`, historia nueva sobre `main`. R-2 alineado al directorio real `specs/`. |
| 1.0.3 | 2026-08-18 | Dependencia actualizada a spec draft.9 (Q-5 en acción durante T-03: PR-20 cadenas, REC_SESSION tipado, REC_STATE, REC-52 enum de tipos, valores numéricos de enums). |

---

## 1. Objetivo

Producir **el codec del protocolo implementado dos veces —Rust y C++— y demostrado equivalente contra un set de vectores compartido**. Nada más.

F0 no produce una herramienta usable. Produce la única pieza que las dos puntas comparten, y la prueba de que no divergen. En v1 el downlink divergió entre firmware y desktop y nadie se enteró porque nunca se ejecutó; F0 existe para que eso no pueda volver a pasar.

### 1.1 Criterios de salida

| # | Criterio |
|---|---|
| S-1 | `codec_vectors.json` existe, valida contra su JSON Schema y cubre la matriz de §6 |
| S-2 | El codec de Rust codifica y decodifica todos los vectores, ida y vuelta |
| S-3 | El codec de C++ codifica y decodifica los mismos vectores, compilado y corrido **en host** |
| S-4 | Los 12 vectores ancla fueron calculados por un tercer camino independiente y coinciden |
| S-5 | Los vectores `must_fail` son rechazados por ambas implementaciones, sin panic ni UB |
| S-6 | 24 h de fuzzing sobre el decoder de Rust sin panic |
| S-7 | El formateador de llaves pasa su propio set de vectores (TEST-08) |
| S-8 | CI corre S-2, S-3, S-5 y S-7 en cada push |

### 1.2 No-objetivos de F0

No hay transporte, ni serie, ni USB, ni tarea de FreeRTOS, ni ring buffers, ni UI, ni store, ni Tauri. No se compila nada para el ESP32: el C++ se compila **para el host**. La API pública de §8.5 no se implementa; F0 toca solo la capa de codificación que está debajo.

---

## 2. Repositorio

**R-1** — Historia limpia: **v1 se elimina por completo**, sin tag de archivo ni rama de transición. Se trabaja sobre `main` como única rama, con `git init` en el directorio de trabajo actual (que ya contiene `specs/`). Nada de v1 se migra ni queda referenciado; si el remoto viejo se conserva o se borra es una decisión fuera de este plan.

**R-2** — Layout objetivo (F0 crea solo lo marcado con ✓):

```
mole/
├── specs/
│   ├── mole-spec.md                    ✓ (ya existe)
│   └── mole-f0-plan.md                 ✓ (este documento)
├── protocol/
│   ├── codec_vectors.json              ✓ artefacto compartido
│   ├── codec_vectors.schema.json       ✓ JSON Schema del formato
│   ├── anchors.py                      ✓ tercer camino independiente
│   └── README.md                       ✓ cómo regenerar y por qué
├── host/
│   ├── Cargo.toml                      ✓ workspace
│   ├── mole-codec/                     ✓ crate: COBS, CRC32, frame, records, args
│   ├── mole-fmt/                       ✓ crate: formateador de llaves
│   ├── mole-vectors/                   ✓ bin: genera y valida codec_vectors.json
│   └── fuzz/                           ✓ targets de cargo-fuzz
├── firmware/
│   └── mole/
│       ├── src/mole_codec.cpp          ✓
│       ├── include/mole_codec.h        ✓
│       ├── include/mole_wire.h         ✓ opcodes, layouts, constantes
│       └── test/                       ✓ CMake, compila para host
├── .github/workflows/ci.yml            ✓
└── CLAUDE.md                           ✓
```

**R-3** — `mole_wire.h` es **solo declaraciones**: opcodes, tamaños, constantes, enums de tipo. Sin lógica. Es lo que después consume tanto el codec como la API pública de F1, y lo que hay que mirar cuando se compara contra §7.1.

**R-4** — Licencia MIT (PA-09). `LICENSE` en la raíz, encabezado SPDX en cada fuente de `firmware/mole/`.

---

## 3. El problema de la fuente de verdad

**V-1** — Si los vectores los genera el codec de Rust y el de C++ se valida contra ellos, **un error en Rust se convierte en el estándar** y las dos implementaciones coinciden en estar mal. Es el riesgo central de F0 y hay que atacarlo explícitamente.

**V-2** — Estrategia de tres caminos:

1. **Vectores ancla (12)**: calculados a mano y verificados con `protocol/anchors.py`, un script escrito **de forma independiente**, leyendo §6 y §7 y no el código de Rust. Deliberadamente ingenuo y lento: sin optimizaciones, sin abstracciones, transcripción directa de la especificación.
2. **Vectores derivados (el resto)**: generados por `mole-vectors` con el codec de Rust.
3. **Verificación cruzada**: `anchors.py` recalcula todos los vectores derivados que estén dentro de su alcance y falla si difiere.

**V-3** — Los 12 ancla se eligen para cubrir cada primitiva al menos una vez y se documentan en `protocol/README.md` con el cálculo escrito paso a paso. Si alguna vez hay una discrepancia, ese documento es el árbitro.

**V-4** — `anchors.py` es un descendiente directo de `analyze_dump.py` de v1, que existía justamente porque hacía falta calcular a mano un paquete para entender por qué no andaba. Esta vez el cálculo a mano es parte del proceso, no un rescate de emergencia.

---

## 4. Formato del archivo de vectores

**V-5** — Un solo archivo, `codec_vectors.json`, con un JSON Schema al lado. Cada vector es autocontenido y declara su capa, de modo que un test puede filtrar por capa y correr solo lo que le toca.

```json
{
  "protocol_version": 2,
  "generated_by": "mole-vectors 0.1.0",
  "vectors": [
    {
      "id": "cobs-004",
      "layer": "cobs",
      "desc": "payload con cero intercalado",
      "decoded_hex": "11002233",
      "encoded_hex": "02110322 33"
    },
    {
      "id": "crc32-001",
      "layer": "crc32",
      "desc": "convención de esp_rom_crc32_le",
      "data_hex": "01020304",
      "crc_hex": "b63cfbcd"
    },
    {
      "id": "frame-007",
      "layer": "frame",
      "desc": "dos records, seq con wrap",
      "frame": {
        "ver": 2, "flags": [], "seq": 65535, "t_base_us": 1234567890,
        "records": ["rec-log-fmt-002", "rec-watch-001"]
      },
      "pre_cobs_hex": "...",
      "wire_hex": "..."
    },
    {
      "id": "rec-log-fmt-002",
      "layer": "record",
      "desc": "log con dos escalares",
      "record": {
        "type": "REC_LOG_FMT", "dt_us": 1500,
        "level": 2, "task_id": 3, "core": 1, "tag_sym": 12, "fmt_id": 7,
        "args": [{"t": "i32", "v": 1024}, {"t": "f32", "v": 2.47}]
      },
      "bytes_hex": "..."
    },
    {
      "id": "fail-003",
      "layer": "frame",
      "must_fail": "crc_mismatch",
      "wire_hex": "..."
    }
  ]
}
```

**V-6** — Reglas del formato:

- `layer` ∈ `cobs | crc32 | record | frame | args | catalog`.
- Los vectores de `frame` referencian records **por id**, para que un cambio en un record se propague solo.
- `must_fail` lleva un motivo enumerado: `crc_mismatch`, `cobs_invalid`, `cobs_underrun`, `len_overflow`, `truncated`, `unknown_type`, `arg_count_mismatch`, `depth_exceeded`.
- Todo hex en minúsculas, sin separadores, sin prefijo.
- Los flotantes se expresan también como `bits_hex` para que no haya ambigüedad de redondeo al parsear el JSON.
- El archivo se ordena por `id` de forma estable, para que los diffs de git sean legibles.

**V-7** — El archivo se **commitea**. No se regenera en CI: CI lo valida. Si un cambio de código lo modifica, eso tiene que aparecer en el diff del PR y discutirse.

---

## 5. Tareas

Orden de ejecución. Cada tarea termina en un commit y tiene una condición de terminado verificable.

| # | Tarea | Termina cuando |
|---|---|---|
| T-01 | `git init` sobre `main`, esqueleto de R-2, `LICENSE`, `CLAUDE.md` | El repo compila vacío y CI corre en verde sin tests |
| T-02 | `codec_vectors.schema.json` según §4 | Un JSON de ejemplo valida; uno malformado falla |
| T-03 | `protocol/anchors.py` + los 12 ancla + `README.md` con el cálculo a mano | Los 12 ancla están calculados y documentados |
| T-04 | `mole-codec`: COBS encode/decode | Ancla de COBS en verde; propiedad ida y vuelta con `proptest` |
| T-05 | `mole-codec`: CRC32 (ver riesgo Q-1) | Ancla de CRC en verde; convención fijada en el vector |
| T-06 | `mole-codec`: header de frame, `seq`, `t_base_us`, detección de huecos | Ancla de frame en verde |
| T-07 | `mole-codec`: header de record y tabla de tipos de §7.1 | Todo tipo de §7.1 se codifica y decodifica |
| T-08 | `mole-codec`: empaquetado de argumentos — escalares, cadenas internadas, cadenas de runtime | Vectores de `args` en verde |
| T-09 | `mole-codec`: structs — `REC_TYPE_DEF`, modo valores, modo bytes, anidados, profundidad 4 | Vectores de struct en verde, incluido padding |
| T-10 | `mole-codec`: catálogo — registros de símbolo, tipo y formato; `epoch`, `catalog_hash`, resync | Vectores de `catalog` en verde |
| T-11 | `mole-fmt`: formateador de llaves | Vectores de TEST-08 en verde, incluido `arg_count_mismatch` |
| T-12 | `mole-vectors`: genera y valida `codec_vectors.json`; verificación cruzada contra `anchors.py` | `codec_vectors.json` commiteado y validado por los dos caminos |
| T-13 | `firmware/mole`: `mole_wire.h` y `mole_codec.cpp`, compilables para host | Compila con CMake fuera del ESP32 |
| T-14 | Runner de tests de C++ que consume `codec_vectors.json` | Todos los vectores en verde en C++ |
| T-15 | `fuzz`: target de decode de frame | 24 h sin panic; corpus semilla desde los vectores |
| T-16 | CI: Rust + C++ + validación del schema en cada push | Verde |

**T-17 (opcional, al final)** — Un `molectl vectors --explain <id>` que imprime el desglose byte a byte de un vector. Es la herramienta que se va a querer tener el día que algo no cierre en F1.

---

## 6. Matriz de cobertura

Los vectores DEBEN cubrir, como mínimo:

### 6.1 COBS
Vacío; un byte; sin ceros; con cero al inicio; con cero al final; ceros consecutivos; bloque de exactamente 254 no-ceros (escape a `0xFF`); 255 no-ceros; payload de `MOLE_FRAME_MAX`.

### 6.2 CRC32
Vacío; un byte; el vector canónico `"123456789"`; payload de 4 KB. **La convención de pre/post inversión queda fijada por estos vectores** (Q-1).

### 6.3 Frame
Un record; `rec_count` máximo; `seq` = 0, 65534, 65535, 0 (wrap); cada flag por separado y combinados; frame vacío (`rec_count` = 0); `t_base_us` cerca del límite de 64 bits.

### 6.4 Records
Uno de cada tipo de §7.1, incluidos `REC_FMT_DEF`, `REC_TYPE_DEF`, `REC_SPAN_ABORT`. `dt_us` = 0, 1, 65535. `len` = 0 y 255.

### 6.5 Argumentos
Cada tipo escalar en su valor mínimo, máximo, cero y negativo. `f32`/`f64` con `NaN`, `+Inf`, `-Inf`, `-0.0`, subnormal. Cadena literal internada. Cadena de runtime vacía, de 1 byte, de 255 bytes. UTF-8 válido e inválido (el host DEBE hacer `from_utf8_lossy`, nunca fallar).

### 6.6 Structs
Struct plano; con padding entre campos; con padding al final; anidado a profundidad 2, 3 y 4; con enum descripto y sin describir; con arreglo; con cadena de runtime dentro (fuerza decodificación secuencial, REC-46); struct cuyo empaquetado supera 255 bytes y se promueve a blob.

### 6.7 `must_fail`
CRC alterado en un bit; COBS con código `0x00` interno; COBS con underrun; `len` de record que excede el frame; frame truncado; `type` desconocido (DEBE decodificar como `Unknown`, no fallar); `REC_LOG_FMT` cuyo `REC_FMT_DEF` nunca llegó; struct con profundidad 5; `arg_count` en desacuerdo con el `REC_FMT_DEF`.

---

## 7. Riesgos y preguntas a resolver durante F0

**Q-1 — Convención de CRC32 de `esp_rom_crc32_le`.** La función de ROM opera con semántica invertida: el uso habitual es `~esp_rom_crc32_le(~0, buf, len)`. Es una fuente clásica de desalineación entre implementaciones. **Acción**: fijar la convención en T-05 con un vector explícito y dejarla escrita en `protocol/README.md`. Es el primer lugar donde mirar si algo no cuadra en F1.

**Q-2 — Byteswap de campos big-endian. RESUELTO (spec draft.8): el host invierte.** El MCU empaqueta los bytes tal como los leyó; `REC_TYPE_DEF` lleva `flags: u8` por campo (bit 0 = big-endian) y la nota quedó en REC-45. Los vectores de struct de §6.6 DEBEN incluir al menos uno con campos big-endian.

**Q-3 — Padding en modo valores.** El modo fiel a los valores empaqueta secuencialmente sin padding, así que el host **no puede** usar `offset` para decodificar: tiene que recorrer los campos en orden acumulando tamaños. El `offset` del `REC_TYPE_DEF` sirve solo para el modo bytes. **Acción**: que quede explícito en el README y en un vector que lo demuestre.

**Q-4 — Instanciación de templates.** No se mide en F0 (no hay compilación para ESP32), pero el diseño del empaquetado de C++ debe minimizar el código generado por combinación de tipos: `put()` no-template por tipo, template solo en el pack. Se mide en F1 contra PERF-15.

**Q-5 — Hallazgos que toquen §6 o §7.** Están congeladas. Si F0 encuentra que algo del formato no cierra, **no se parchea el código**: se abre una nota, se corrige la spec en un draft.8 y se regeneran los vectores. Que aparezca uno o dos es esperable y sano; es exactamente para lo que sirve escribir el codec antes que todo lo demás.

---

## 8. Notas para Claude Code

**C-1** — `mole-spec.md` §6 y §7 son la autoridad. Ante cualquier ambigüedad entre la spec y este plan, gana la spec; si la spec es ambigua, **parar y preguntar**, no elegir una interpretación y seguir.

**C-2** — `anchors.py` se escribe **leyendo la spec, sin mirar el código de Rust**. Si se escribe por transcripción del codec, pierde todo su valor como tercer camino (V-1). Ingenuo y verboso es correcto acá.

**C-3** — Sin `unsafe` en `mole-codec` salvo justificación escrita en el commit. Sin `panic!`, `unwrap()` ni `expect()` en el camino de decodificación: todo devuelve `Result`. Es entrada no confiable (SEC-04).

**C-4** — El C++ de F0 no incluye `Arduino.h` ni ningún header de ESP-IDF. Compila con `g++` sobre el host. Sin asignación dinámica en el codec.

**C-5** — Un commit por tarea, con el código de tarea en el mensaje (`T-04: COBS encode/decode`).

**C-6** — Los tests de C++ y los de Rust leen **el mismo archivo**. Ninguna de las dos implementaciones tiene su propio set de casos para el codec.

**C-7** — `CLAUDE.md` de la raíz bajo 1.500 tokens: qué es el proyecto, dónde está la spec, la regla de que §6/§7 están congeladas, la estructura del repo y las convenciones C-3 a C-6. Nada más; lo demás se lee de la spec.

---

## 9. Qué habilita F0

Terminado F0, F1 arranca sin ambigüedades de formato y con dos implementaciones que ya se hablan. El firmware de banco de F1 —el que emite a tasa creciente para medir §4— se apoya directo sobre `mole_codec.cpp`, y `molectl` sobre `mole-codec`. La primera medición real de PERF-04 a PERF-06 pasa a estar a una tarea de distancia.
