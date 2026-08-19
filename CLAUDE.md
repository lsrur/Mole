# Mole v2

Herramienta de prototipado interactivo y debugging de firmware ESP32: librería C++ (`firmware/`) + host en Rust (`host/`). El repo está en **F0**: el codec del protocolo implementado dos veces —Rust y C++— y demostrado equivalente contra un set de vectores compartido. Nada más que eso todavía: sin transporte, sin FreeRTOS, sin UI. El C++ se compila **para el host**, nunca para el ESP32.

## Autoridad

- `specs/mole-spec.md` (la versión vigente está en su frontmatter) es **la autoridad**. §6 (framing/protocolo) y §7 (records) están **CONGELADAS**: si algo del formato no cierra, no se parchea el código — se abre una nota, se corrige la spec en un draft nuevo y se regeneran los vectores (Q-5 del plan).
- `specs/mole-f0-plan.md` es el plan en ejecución (tareas T-01..T-16).
- Ante ambigüedad entre spec y plan, gana la spec. Si la spec es ambigua, **parar y preguntar**, no elegir una interpretación y seguir.

## Estructura

```
specs/            spec y plan (la autoridad)
protocol/         codec_vectors.json + schema + anchors.py (tercer camino independiente)
host/             workspace Rust: mole-codec, mole-fmt, mole-vectors, fuzz
firmware/mole/    codec C++: mole_wire.h (solo declaraciones), mole_codec.{h,cpp}, test/
```

## Convenciones

- Sin `unsafe` en `mole-codec` salvo justificación escrita en el commit. Sin `panic!`, `unwrap()` ni `expect()` en el camino de decodificación: todo devuelve `Result`. Es entrada no confiable.
- El C++ no incluye `Arduino.h` ni headers de ESP-IDF. Compila con `g++`/CMake en host. Sin asignación dinámica en el codec.
- Los tests de Rust y de C++ leen **el mismo** `protocol/codec_vectors.json`. Ninguna implementación tiene su propio set de casos.
- `codec_vectors.json` se commitea; CI lo valida, no lo regenera. Si un cambio de código lo modifica, tiene que verse en el diff del PR.
- `anchors.py` se escribe leyendo la spec, **sin mirar el código de Rust**. Ingenuo y verboso es correcto ahí.
- Un commit por tarea, con el código en el mensaje: `T-04: COBS encode/decode`.
- Licencia MIT; encabezado SPDX en los fuentes de `firmware/mole/`.
