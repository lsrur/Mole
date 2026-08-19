---
doc: mole-f1-plan.md
version: 0.2.0
fecha: 2026-08-18
estado: PARA EJECUTAR (frente host; el frente hardware espera la placa)
depende_de: mole-spec.md v2.0.0-draft.11 (§6 y §7 congeladas); F0 completo
ejecutor: Claude Code
---

# Mole v2 — Plan de F1: firmware core y `molectl`

## Changelog

| Versión | Fecha | Cambios |
|---|---|---|
| 0.1.0 | 2026-08-18 | Borrador inicial. |
| 0.2.0 | 2026-08-18 | Q-1..Q-4 resueltas: sin placa todavía (frente host arranca, T-08/09/12/13/14 esperan una ESP32-S3 con USB nativo); IDF pineado a v5.5.x; `serialport-rs`; sin instrumento (PERF-03 pendiente formal, PERF-01/02 por ciclos). |

---

## 1. Objetivo

Producir **el camino de datos completo firmware→host, medido**: la librería de firmware con logs diferidos, watch y counters corriendo sobre la `moleTask` real, y el host headless `molectl` recibiendo, decodificando y contando. Nada de UI.

El criterio que gobierna todo (spec §16): **PERF-04/05/06 medidos en hardware real**. Si los números no llegan, se ajusta la spec explícitamente (PERF-13) — pero primero se mide. F1 es la puerta verde de la que depende el resto: si el core no llega a los números de §4, lo de arriba se rediseña.

### 1.1 Criterios de salida

| # | Criterio | Ref |
|---|---|---|
| S-1 | PERF-04 (≥600 KB/s USB-CDC), PERF-05 (≥180 KB/s UART@2M), PERF-06 (≥50k rec/s sin pérdida) **medidos** con `molectl bench` y anotados | §16, PERF-13 |
| S-2 | PERF-01/02 (<1 µs log/watch) y PERF-14 (<2 µs struct) medidos por contador de ciclos en S3 | TEST-07* |
| S-3 | TEST-04: emitidos = recibidos + drops reportados. **Ninguna pérdida no contabilizada** | FEAT-34 |
| S-4 | Late-join: abrir `molectl` con el MCU andando converge en ≤2 s sin reset | FEAT-33, CAT-11 |
| S-5 | `MOLE_ENABLED=0` compila a nada: sin strings ni símbolos en el binario, y PERF-15 (≥4 KB de flash sin float-printf) medido | FEAT-47 |
| S-6 | PERF-07 (ráfaga ≥8k records) y PERF-12 (<24 KB RAM) medidos | §4 |
| S-7 | Los bytes que emite el firmware validan contra `codec_vectors.json` (los dos codecs siguen sin divergir) | C-6 |
| S-8 | CI compila el componente para ESP32-S3 y corre los tests de host en cada push | TEST-02 |

*PERF-03 (jitter con GPIO + osciloscopio) queda condicionado a tener el instrumento — ver Q-4.

### 1.2 No-objetivos de F1

Sin UI, sin Tauri, sin store columnar (F2). Sin bind ni comandos (F3) — pero el **downlink sí existe** en su mínimo: `CTL_HELLO`/resync, `CTL_PING`, `CTL_SET_LEVEL` y `CTL_SET_POLICY`, porque el late-join (S-4) y sostener PERF-06 (PR-19) los necesitan. Sin dumps con schema (F4; `MOLE_DESCRIBE` entra igual porque el logueo de structs es F1 — FEAT-52). Sin spans (F5), sin pausa (F6), sin sesiones (F7), sin TCP (F8). Transporte USB-Serial-JTAG (TR-03): solo si sobra tiempo; el target de referencia es S3.

---

## 2. Prerequisitos (bloquean las tareas de hardware, no las de host)

**P-1 — Hardware.** Una placa **ESP32-S3 con el conector USB nativo accesible** (ej. DevKitC-1, que trae los dos puertos: UART-bridge y USB-OTG). Para PERF-05 hace falta además un puente USB-serial que banque 2 Mbaud (el CP2102 del DevKit sirve). Hoy no hay ninguna placa conectada. **Ver Q-1.**

**P-2 — Toolchain.** El IDF local es `v6.0-dev` (snapshot de desarrollo). La librería DEBE construirse contra un **release estable pineado** — propuesta: `v5.5.x` LTS, que es lo que además corre debajo de Arduino-ESP32 v3 (PLAT-03). El dev-snapshot puede quedar para detectar roturas futuras, pero los números de §4 se miden sobre el release. **Ver Q-2.**

**P-3 — Instrumento.** PERF-03 y la validación externa de PERF-01/02 piden analizador lógico u osciloscopio (TEST-07). Sin instrumento, F1 mide por contador de ciclos (`esp_cpu_get_cycle_count()`) y PERF-03 queda pendiente formal. **Ver Q-4.**

---

## 3. Estructura

**R-1** — Lo que F1 agrega al árbol (✓ = crea, ± = amplía):

```
firmware/mole/
├── CMakeLists.txt              ✓ componente ESP-IDF
├── Kconfig                     ✓ MOLE_RING_SIZE, MOLE_FRAME_MAX, MOLE_FLUSH_MS, ...
├── library.json                ✓ PlatformIO (mismo árbol, FW-02)
├── include/
│   ├── mole.h                  ✓ API pública de F1 (§8.5, subconjunto)
│   ├── mole_config.h           ✓ defaults y compilación condicional (FW-11..13)
│   ├── mole_codec.h            ± (F0)
│   └── mole_wire.h             ± (F0)
├── src/
│   ├── mole_core.cpp           ✓ intern, rings SPSC, políticas, registro perezoso
│   ├── mole_task.cpp           ✓ moleTask: drenaje, framing, TX, RX, resync
│   ├── mole_codec.cpp          ± (F0)
│   ├── mole_transport.h        ✓ IMoleTransport (TR-07)
│   ├── mole_transport_uart.cpp ✓ TR-01
│   └── mole_transport_cdc.cpp  ✓ TR-02 (TinyUSB)
├── examples/bench/             ✓ firmware de banco (TEST-04/TEST-05)
└── test/                       ± tests de host: se amplía con core y macros

host/
└── molectl/                    ✓ bin: transporte serie, decoder, bench, capture cruda
```

**R-2** — **El core se parte en dos capas para poder testearlo en host** (TEST-02): todo lo que no toca FreeRTOS/IDF (intern, rings, políticas, registro perezoso, empaquetado de las macros) vive detrás de un par de hooks (`mole_now_us()`, `mole_task_slot()`) que en el target son `esp_timer_get_time()`/TLS de FreeRTOS y en host son mocks. **No es un HAL** (PLAT-05 sigue firme): son dos funciones de test, no una capa de abstracción.

**R-3** — El codec **no se reimplementa**: `mole_task.cpp` arma frames con el `FrameWriter` de F0 y parsea downlink con `frame_parse_header`/`frame_records`. Cualquier byte nuevo que emita el firmware tiene que validar contra `codec_vectors.json` (S-7).

---

## 4. Tareas

Un commit por tarea (C-5). Las tareas 02–07 corren enteras en host; el hardware entra recién en T-08.

| # | Tarea | Termina cuando |
|---|---|---|
| T-01 | Componente ESP-IDF (`CMakeLists.txt`, `Kconfig`) + `library.json`, compilando vacío para `esp32s3` contra el IDF pineado (P-2) | `idf.py build` del ejemplo vacío en verde; tests de host de F0 siguen en verde |
| T-02 | `intern()` y tabla de símbolos: ids secuenciales (CAT-01/02), kinds (CAT-03), `SYM_OVERFLOW` con un solo `REC_STATS` y **sin reintento en loop** (CAT-05, bug de v1), emisión de `REC_SYM_DEF` | Tests de host: colisión imposible, overflow contabilizado una vez, bytes contra vectores |
| T-03 | Rings SPSC por tarea productora (FW-04) + ring de ISR (FW-05) + políticas `Block`/`DropNewest`/`DropOldest`/`Decimate` (PR-12) + `MOLE_BLOCK_TIMEOUT_MS` (FW-08) + contadores de descarte por canal | Tests de host multihilo: sin locks en el hot path, descarte contado exacto bajo presión |
| T-04 | Macros de log diferido: `MOLE_TRACE..FATAL` y `_T`, conteo de `{}` contra `sizeof...(Args)` en compilación (REC-33), registro perezoso de `REC_FMT_DEF` (REC-34), empaquetado de args según REC-52, cadenas por sobrecarga literal/runtime (REC-38), `mole::logs()` fallback (REC-36) | Tests de host: bytes idénticos a los vectores `rec-fmt-def`/`rec-log-fmt`; el conteo de llaves malo **no compila** (test negativo con `static_assert`) |
| T-05 | `MOLE_DESCRIBE`/`_BE`/`_ENUM`/`MOLE_FIELD_ARRAY` → `TypeDesc` `constexpr` por ADL (REC-42), emisión perezosa de `REC_TYPE_DEF`/`REC_ENUM_DEF` (REC-43/53), empaquetado modo valores (REC-45), `MOLE_BYTES` (REC-48) | TEST-09 en host: `offsetof`/`sizeof` reales con padding, anidados, arreglos y enums; bytes contra `rec-type-def-imu`, `rec-enum-def`, `args-struct-*` |
| T-06 | `watch()`/`watchEvery()` (REC-06/09, descarte en el productor), `count()` seguro desde ISR (REC-24) con agregación y `REC_COUNTER` batcheado cada 250 ms (REC-23), `event()` | Tests de host: rate-limit exacto, counters coalescidos, bytes contra vectores |
| T-07 | `moleTask` en host simulado: drenaje round-robin (FW-06), `FrameWriter` con cierre por tamaño/tiempo/urgencia (PR-05, PR-08), `REC_STATS` cada 500 ms y ante cambio de drops (PR-13), `FLAG_DROPS`/`FLAG_CATALOG` | Test de host end-to-end: N productores sintéticos → frames que decodifica `mole-codec` (Rust) sin pérdida no contabilizada |
| T-08 | Transportes reales: `IMoleTransport` + UART (TR-01, hasta 2 Mbaud) + USB-CDC TinyUSB (TR-02) con `tud_cdc_connected()` (TR-08: sin host, descarte gratis en el productor) | El ejemplo bench corre en la placa y `molectl raw` muestra frames válidos por ambos transportes |
| T-09 | Downlink mínimo: parseo con CRC en `moleTask` (PR-15..17), `CTL_HELLO`→`REC_SESSION` + resync completo (CAT-07..11, epoch en RTC memory), `REC_SESSION` no solicitado cada 2 s (CAT-11), `CTL_PING`→`REC_PONG`, `CTL_SET_LEVEL` (PR-19), `CTL_SET_POLICY`, `CTL_RESET` con magic (SEC-02, SEC-03) | S-4 demostrado: `molectl` abierto tarde reconstruye catálogo en ≤2 s; un byte de ruido no resetea (test de ruido) |
| T-10 | `MOLE_ENABLED=0` (FW-12) y niveles de compilación (FW-13); medición PERF-15 | `strings` sobre el ELF release no contiene ningún literal de instrumentación; delta de flash documentado |
| T-11 | `molectl`: crate binario sobre `mole-codec` — apertura de puerto (autodetección VID/PID básica), splitter por `0x00`, decoder por lotes (HOST-03: ≥500k rec/s en un core, benchmark propio), canal raw (HOST-05), contabilidad de `seq` (PR-06) y de drops del MCU | `molectl watch <puerto>` muestra records decodificados y contadores de salud en vivo; benchmark de decoder documentado |
| T-12 | `molectl bench` (TEST-05): orquesta el firmware de banco, reporta la tabla de §4 y compara contra `bench_baseline.json` commiteado (falla si degrada >10%) | Corrida reproducible con salida en JSON + tabla |
| T-13 | Firmware de banco (`examples/bench`): emite a tasa creciente por canal (log/watch/counter), con contadores propios de emisión para TEST-04; modos ráfaga (PERF-07) y sostenido | S-3: emitidos = recibidos + drops, verificado por `molectl bench` en las dos puntas |
| T-14 | **Medición**: PERF-04/05/06/07/12 con `molectl bench`; PERF-01/02/14 por contador de ciclos promediado; resultados anotados en `bench_baseline.json` y, si algo no llega, en la spec (PERF-13) | S-1/S-2/S-6 cerrados con números reales escritos |
| T-15 | CI: job de build del componente (espressif/esp-idf-ci-action, target esp32s3) + tests de host ampliados al core | Verde en push |

**T-16 (opcional)** — Smoke de PLAT-03: el componente compila bajo PlatformIO con framework Arduino-ESP32 v3 (sin correr en placa). Barato y detecta el 90% de los problemas de compatibilidad de headers.

---

## 5. Medición: cómo se produce cada número

**M-1** — PERF-04/05 (throughput): el bench emite logs de tamaño realista (dos escalares, REC-04) a tasa creciente hasta saturar; `molectl` mide KB/s sostenidos durante ≥30 s descontando rampa. USB-CDC y UART@2M por separado.

**M-2** — PERF-06 (rec/s sin pérdida): igual, pero el criterio es `dropped == 0` en `REC_STATS` y cero huecos de `seq` durante 60 s a la tasa objetivo.

**M-3** — PERF-07 (ráfaga): el bench encola 8.000 records con el transporte artificialmente frenado 50 ms y verifica cero drops tras drenar.

**M-4** — PERF-01/02/14 (costo por llamada): loop de 10k llamadas entre lecturas de `esp_cpu_get_cycle_count()` con la cola vacía, p50 y p99, a 240 MHz. Es medición interna: cuando haya instrumento (Q-4), se contrasta con GPIO (TEST-07).

**M-5** — PERF-12 (RAM): `heap_caps_get_info()` antes y después de `mole::begin()` + high-water de la config por defecto con 3 productores.

**M-6** — PERF-15 (flash): `idf.py size` de dos builds del bench — formateo diferido vs. un build de control con un `printf("%f")` — delta documentado.

---

## 6. Riesgos y preguntas

**Q-1 — ~~¿Qué hardware hay?~~ RESUELTO (0.2.0): ninguno todavía.** Se ejecuta el **frente host** (T-01..T-07, T-10, T-11 salvo su prueba con puerto real, T-15, T-16). T-08, T-09, T-12, T-13 y T-14 quedan bloqueadas hasta enchufar una **ESP32-S3 con USB nativo accesible** (recomendación: DevKitC-1, trae los dos puertos). El código de esas tareas puede escribirse antes; sus condiciones de terminado exigen la placa.

**Q-2 — ~~Versión de IDF.~~ RESUELTO (0.2.0): pineado a `v5.5.x`.** El `v6.0-dev` local queda fuera de la base de medición.

**Q-3 — ~~Crate serial.~~ RESUELTO (0.2.0): `serialport-rs`.** `nusb` recién si PA-03 se abre después de medir.

**Q-4 — ~~Instrumento.~~ RESUELTO (0.2.0): no hay.** PERF-01/02/14 se miden por contador de ciclos; PERF-03 y la validación por GPIO (TEST-07) quedan **pendientes formales** anotadas en la spec hasta que haya analizador u osciloscopio.

**Q-5 — PERF-01 en la práctica.** <1 µs son ~240 ciclos: entra un `load` del sym estático + empaquetado a ring SPSC sin contención, pero no sobra nada. Si el p99 se pasa por el camino de registro perezoso (primera ejecución), la medición DEBE excluir la primera llamada — el presupuesto es de régimen. Si aún así no llega, PERF-13: se anota el número real y se decide con datos.

**Q-6 — Hallazgos que toquen §6/§7.** Siguen congeladas (draft.11). Mismo mecanismo que en F0: nota → draft nuevo → vectores regenerados. F1 es el primer consumidor real del protocolo; que aparezca algo es posible y sano.

---

## 7. Notas para Claude Code

**C-1** — La spec manda; este plan ejecuta. Ambigüedad en la spec ⇒ parar y preguntar (igual que F0).

**C-2** — Los bytes que emite el firmware se validan contra `codec_vectors.json`, no contra expectativas escritas a mano nuevas. Si hace falta un caso que no existe, se agrega el vector primero (por `mole-vectors`) y las dos implementaciones lo consumen.

**C-3** — Hot path: sin mutex, sin asignación dinámica, sin formateo. Todo lo caro vive en la `moleTask` o en el host. Cualquier excepción se justifica en el commit.

**C-4** — Los tests de host del core no incluyen headers de IDF (C-4 de F0 sigue). Los dos hooks de R-2 son la única costura.

**C-5** — Un commit por tarea, código de tarea en el mensaje (`F1-T04: macros de log diferido`). Prefijo `F1-` para no colisionar con los T-xx de F0 en el historial.

**C-6** — Nada de números inventados: toda celda de la tabla de §4 que se declare medida tiene que salir de una corrida reproducible con `molectl bench` (TEST-05) o del procedimiento de §5.

---

## 8. Qué habilita F1

Con F1 verde, F2 (desktop: store columnar, coalescer, vistas de Log/Watch) arranca sobre un enlace medido y un `molectl` que ya hace de banco de pruebas sin UI. Y PA-03 (USB vendor bulk) se decide con el número de PERF-04 en la mano, no con intuición.
