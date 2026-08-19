---
doc: mole-f2-plan.md
version: 0.2.0
fecha: 2026-08-19
estado: PARA EJECUTAR (frente A ya; frente B con PA-12 resuelto; cierre formal con la placa)
depende_de: mole-spec.md v2.0.0-draft.12; F0 completo; F1 con gate de números pendiente de placa
ejecutor: Claude Code
---

# Mole v2 — Plan de F2: desktop — núcleo Rust, Log y Watch

## Changelog

| Versión | Fecha | Cambios |
|---|---|---|
| 0.1.0 | 2026-08-19 | Borrador inicial. Abiertos: PA-12 (framework), PA-06 (retención). |
| 0.2.0 | 2026-08-19 | PA-12 y PA-06 resueltos por spec draft.12: **Vue 3 + Ark UI + dockview + TanStack Virtual** y **descarte con aviso**. B-03 arranca con el spike del popout de dockview bajo Tauri; B-04 anota que TanStack no exime el scrollbar indexado (UI-25). Gate: frente A arranca ya; el cierre formal de F2 espera los números de F1. |

---

## 1. Objetivo

La primera versión **usable** de Mole: la app desktop que recibe el stream, lo guarda en el store columnar de Rust, y muestra **Log y Watch** de verdad — filtrado instantáneo, follow/freeze, modos de tiempo, watch con sparklines, salud del enlace siempre visible. Sin dumps (F4), sin spans (F5), sin binds ni comandos (F3).

La decisión de la que depende todo el presupuesto es ARQ-02: **el estado vive en Rust, la UI es una vista**. Nada en la UI hace trabajo proporcional al volumen (UI-03). El fantasma a exorcizar es el patrón de v1: un `emit()` de IPC por record y un array de mensajes en JS.

### 1.1 Criterios de salida

| # | Criterio | Ref |
|---|---|---|
| S-1 | PERF-11: filtrar 10M líneas por tag+texto en <300 ms, medido con benchmark reproducible | HOST-09 |
| S-2 | PERF-10: <1,5 GB de RSS con 10M records en el store | HOST-07 |
| S-3 | PERF-09: UI a 60 fps con ingesta sintética de 50k rec/s, sin frames caídos | UI-03 |
| S-4 | PERF-08: latencia MCU→pantalla p95 <120 ms — **requiere placa**; se mide al cerrar el gate de F1 | §4 spec |
| S-5 | El pipeline completo corre sin UI (ARQ-03): los benchmarks de S-1/S-2 no abren ventana alguna | ARQ-03 |
| S-6 | Log: columnas UI-12, modos de tiempo UI-13, filtros siempre visibles UI-14, búsqueda UI-17, resaltado UI-16, marcadores UI-18, follow/freeze UI-19, filas especiales UI-20, copiado UI-22 | §10.4 |
| S-7 | Watch: tabla UI-27 con stats incrementales correctas (el promedio de v1 se rompía) y agrupación jerárquica UI-28 | §10.6 |
| S-8 | FEAT-03 operable desde la UI: silenciar un tag en vivo viaja por CTL_SET_LEVEL (el firmware ya lo honra, F1-T09) | PR-19 |
| S-9 | CI: tests del núcleo + benchmarks de humo en cada push | — |

### 1.2 Gate de F1

La spec es explícita: *F1 antes que cualquier UI*. Interpretación operativa propuesta: el **frente A (núcleo Rust)** de este plan no es UI y puede ejecutarse ya — sus presupuestos (PERF-09/10/11) no dependen del enlace físico. El **cierre de F2** (S-4 y el baseline real) espera los números de la placa. Si PERF-04/05/06 no llegan y §4 se recalibra (PERF-13), lo que se rediseña es el firmware/transporte — el store y la UI consumen lo que llegue.

---

## 2. Decisiones (resueltas en spec draft.12)

**PA-12 — RESUELTO: Vue 3 + Ark UI + dockview-vue + TanStack Virtual; PrimeVue afuera.** El stack completo con su racional está en §17.2 de la spec. Consecuencias operativas acá: B-03 usa dockview (no splitters a mano) y arranca con el **spike del popout bajo Tauri** (§10 UI-08); B-04 usa TanStack Virtual para la ventana pero el scrollbar indexado de sesión completa es propio (UI-25); el look es de tokens (UI-46), no de librería.

**PA-06 — RESUELTO: descarte con aviso** (spec §17.2). El store pagina desde el día uno; A-02 implementa el descarte paginado con la marca "datos desde".

**Nota PA-10.** El scaffold usa los nombres ya resueltos: `mole-app`, bundle `net.elesis.mole-app`.

---

## 3. Estructura

```
host/
├── mole-host/            ✓ lib: pipeline (transporte→decoder→store→coalescer),
│                            store columnar, filtros, tick, log_slice.
│                            molectl pasa a consumirla (se le extrae pipeline.rs)
├── molectl/              ± suma: replay de archivo como transporte, bench-store
└── mole-app/             ✓ Tauri 2 + [PA-12]
    ├── src-tauri/           comandos IPC: tick 30 Hz + log_slice binario
    └── src/                 shell, splitters a mano, paneles Log y Watch
```

**R-1** — `mole-host` es la biblioteca; `molectl` y `mole-app` son dos frentes del mismo pipeline (HOST-14: todas las ventanas consumen el mismo contrato).

**R-2** — Los paneles calientes (log, scroller, sparklines) se escriben a mano (UI-43); la librería de componentes solo para lo frío y preferentemente headless (UI-44).

---

## 4. Tareas

### Frente A — núcleo Rust (ejecutable ya, sin UI)

| # | Tarea | Termina cuando |
|---|---|---|
| A-01 | `mole-host`: extraer y generalizar el pipeline de molectl; trait `Transport` (TR-07) con impl serial y archivo (replay crudo) | molectl decode-file/watch corren sobre mole-host sin cambio de conducta |
| A-02 | Store columnar de logs: arena paginada de 1 MB, columnas (t, level, tag, task, core, fmt_id) + args tipados; retención PA-06 con descarte paginado avisado | 10M records sintéticos ingeridos; PERF-10 medido |
| A-03 | Watches: ring por símbolo (default 10k), stats incrementales correctas (min/max/media/desvío con Welford) | test contra valores de referencia; el bug del promedio de v1 tiene test de regresión |
| A-04 | Filtros sobre columnas (HOST-09): nivel/tag/tarea/core como bitsets + texto/regex sobre el render perezoso; filtro por valor de argumento (FEAT-51) | PERF-11 medido: 10M por tag+texto <300 ms |
| A-05 | Coalescer y contrato `Tick` (HOST-11) + `log_slice` binario (HOST-12) con ventana e índices bajo filtro | test: 50k rec/s sintéticos → ticks de 33 ms estables, slice correcto bajo filtro activo |
| A-06 | `molectl bench-store`: ingesta sintética + los números S-1/S-2 en JSON contra baseline | corrida reproducible en CI (versión reducida) |

### Frente B — la app (requiere PA-12 decidido)

| # | Tarea | Termina cuando |
|---|---|---|
| B-01 | Scaffold Tauri 2 (`mole-app`, PA-10) + Vue 3 + tokens UI-46 + puente IPC: tick por evento, slice por `invoke` binario | la app muestra rec/s reales de un replay sin tocar JSON por record |
| B-02 | Shell: barra superior con identidad del dispositivo (UI-37), salud del enlace con números (UI-35, PR-14), controles de conexión (UI-34: baudrate solo si UART) | conectar/desconectar/reconectar contra replay y puerto |
| B-03 | **Spike primero**: popout de dockview bajo Tauri (si no anda: ventana Tauri propia, HOST-14). Después: dockview-vue con pestañas, persistencia y presets con atajo (UI-04..08, FEAT-55/56), chrome restyleado con UI-46 | layout arrastrable, guardado y restaurado; veredicto del popout documentado |
| B-04 | Panel Log: TanStack Virtual para la ventana + scrollbar indexado propio (UI-23..26), columnas conmutables (UI-12), modos de tiempo (UI-13), follow/freeze con píldora (UI-19, `followOnAppend`), filas especiales de drops/cortes (UI-20), copiado (UI-22) | S-6 salvo filtros; PERF-09 preliminar con replay a máxima velocidad |
| B-05 | Barra de filtros siempre visible (UI-14) + búsqueda n/N (UI-17) + resaltado por reglas (UI-16) + marcadores y delta-a-marca (UI-18, FEAT-57/58/59) | S-6 completo |
| B-06 | Panel Watch: tabla con stats y formato retroactivo por fila (UI-27), agrupación jerárquica (UI-28), sparklines en canvas (FEAT-09) | S-7 |
| B-07 | Nivel de log en vivo desde la UI (FEAT-03 → CTL_SET_LEVEL) + canal raw (UI-33, HOST-05) | S-8 |
| B-08 | Paleta de comandos `Cmd+K` (UI-38, FEAT-62) + atajos (UI-39) | acciones de paneles/presets/filtros accesibles |
| B-09 | Medición final: PERF-09 con ingesta sintética 50k rec/s (devtools), PERF-08 cuando haya placa | S-3 (y S-4 con hardware) |

---

## 5. Riesgos y preguntas

**Q-1 — El scrollbar indexado y las alturas.** UI-25: con 10M filas el alto real revienta el límite del navegador. El scroller mapea posición→índice a mano. Riesgo conocido, la spec ya lo resolvió en diseño; el plan solo hereda la decisión de no usar un componente de tabla (UI-43).

**Q-2 — Regex sobre 10M líneas.** El texto se renderiza perezosamente (REC-39: el store guarda args tipados, no strings). Para el filtro de texto, el camino rápido es filtrar primero por columnas enteras y regexear solo los candidatos. Si PERF-11 no llega con regex puro, el fallback documentado es substring por defecto y regex opt-in.

**Q-3 — Tauri en CI.** Los tests de UI headless son frágiles; S-9 cubre el núcleo + humo. Los tests de UI reales llegan con fixtures `.molecap` (TEST-06) en F7.

**Q-4 — Node/npm local.** El frente B necesita toolchain de frontend. Verificar versión al arrancar B-01.

---

## 6. Notas para Claude Code

**C-1** — La spec manda; ambigüedad ⇒ parar y preguntar (igual que F0/F1).

**C-2** — Prohibido el patrón v1: ni un `emit()` por record, ni arrays de mensajes en JS, ni componentes de tabla en paneles calientes. El tick de 30 Hz y el slice binario son el único puente.

**C-3** — Todo benchmark que declare un PERF-xx sale de `molectl bench-store` o del procedimiento de §4 del plan de F1; nada estimado.

**C-4** — Prefijo de commits `F2-A01`/`F2-B03`, un commit por tarea.

---

## 7. Qué habilita F2

Con F2, Mole ya sirve: conectás el ESP32 (o un replay) y ves logs filtrables y watches en vivo. F3 (bind + comandos) agrega el ciclo interactivo sobre el downlink que F1 ya dejó andando, y F4 (dumps) completa el caso de uso del sensor I2C.
