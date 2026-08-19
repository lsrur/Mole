---
doc: mole-spec.md
version: 2.0.0-draft.8
fecha: 2026-08-18
estado: BORRADOR — para debate
alcance: protocolo + librería de firmware + aplicación desktop
compatibilidad: NINGUNA con v1.x (ruptura deliberada y total)
---

# Mole v2 — Especificación

## Changelog

| Versión | Fecha | Cambios |
|---|---|---|
| 2.0.0-draft.1 | 2026-08-18 | Documento inicial. Ruptura total con v1. Foco ESP32, transporte USB-CDC nativo, batching de records, catálogo de símbolos secuencial, spans, bind, dump con schema. |
| 2.0.0-draft.2 | 2026-08-18 | Nuevo §1.4: catálogo de funcionalidades (FEAT-01..49). Resueltos PA-02 (solo ESP32), PA-07 (ELF a v3), PA-08 (descartado), PA-09 (MIT), PA-10 (`mole-app`), PA-11 (descartado). |
| 2.0.0-draft.3 | 2026-08-18 | Resuelto PA-05: los spans abiertos se invalidan al pausar, nuevo record `REC_SPAN_ABORT` (0x52). |
| 2.0.0-draft.4 | 2026-08-18 | Resuelto PA-01 a favor del **formateo diferido con llaves**. Reescrito §7.2, nuevos records `REC_FMT_DEF` (0x05) y `REC_LOG_FMT` (0x11), reglas REC-32..REC-39, nuevas FEAT-50/51, macros de log actualizadas en §8.5. |
| 2.0.0-draft.5 | 2026-08-18 | Nuevo §7.2.4: logueo de structs con descriptor no intrusivo (`MOLE_DESCRIBE`). Nuevo record `REC_TYPE_DEF` (0x06), reglas REC-40..REC-49, FEAT-52..54. Resuelto PA-04 por consecuencia: el descriptor es fuente única para logs y para el overlay de dumps, §7.5 reescrita. |
| 2.0.0-draft.6 | 2026-08-18 | §10 reescrita y expandida: layout de splitters con presets, estrategia de renderizado por panel, especificación completa del panel de Log y del scroller virtual, requisitos de stack. Reglas UI-01..UI-45, FEAT-55..62. Nuevo PA-12 (framework). |
| 2.0.0-draft.7 | 2026-08-18 | Pasada de consistencia previa al congelamiento: §8.5 actualizada a las APIs de descriptor y formateo diferido; PERF-01 revisado a la baja y nuevos PERF-14/15; nota sobre numeración de FEAT. §6 y §7 quedan **congeladas**. Abiertos: PA-03, PA-06, PA-12. |
| 2.0.0-draft.8 | 2026-08-18 | Correcciones de wire previas a F0, surgidas de la revisión del plan (mecanismo Q-5): `span_id` pasa a **contador global** de instancias con regla de stale en el host (REC-21); nuevo `flags: u8` por campo en `REC_TYPE_DEF`, bit 0 = big-endian — el MCU nunca invierte, el host invierte al decodificar (REC-43/REC-45, resuelve Q-2 del plan de F0); §7.10 reescrita: los checks usan **formateo diferido**, `REC_CHECK_FAIL` pasa a `{ fmt_id, task_id, core, args[] }`. Retoques editoriales: FEAT-05 (costo en régimen cero) y PLAT-04 (framework de UI remite a PA-12). §6 y §7 quedan **congeladas en este draft**. |

## Convenciones del documento

- Cada regla normativa lleva un código estable (`PR-04`, `FW-11`). Los códigos no se reciclan: si una regla se elimina, queda tachada con nota.
- **DEBE** = requisito duro. **DEBERÍA** = recomendación fuerte. **PUEDE** = opcional.
- Los puntos abiertos están numerados en §17 y se resuelven por debate antes de congelar la versión.
- Los presupuestos de performance (§4) son criterios de aceptación, no aspiraciones.

---

## 1. Objetivo y no-objetivos

### 1.1 Qué es Mole

Una herramienta de **prototipado interactivo y debugging de firmware ESP32**, compuesta por una librería C++ y una aplicación desktop. El caso de uso central es el ciclo corto: conectás un sensor nuevo, un módulo de radio o un bus que nunca usaste, y necesitás *ver qué está pasando adentro* y *tocar parámetros sin recompilar*.

### 1.2 Tesis del producto (OBJ-01)

Las herramientas existentes (Serial Studio, Teleplot, Better Serial Plotter) resuelven **telemetría**: series de tiempo lindas para mostrar. Mole resuelve **introspección de firmware en desarrollo**. El gráfico de una variable es un widget más, no el centro de gravedad.

El centro de gravedad de Mole es:

1. **Logs de altísimo volumen** que no se pierden, se filtran instantáneamente y llevan contexto (tarea, core, archivo:línea, tag, timestamp del MCU).
2. **Inspección de buffers y registros**: hexdump con overlay de struct, decodificación de bitfields, y **diff byte a byte contra el dump anterior**.
3. **Profiling real**: spans anidados con histograma (p50/p95/p99), no `min/max/avg`.
4. **Control bidireccional sin recompilar**: variables ligadas (`bind`) que se editan desde el desktop en vivo, y comandos declarados desde el firmware.
5. **Pausa cooperativa** compatible con FreeRTOS.

### 1.3 No-objetivos (OBJ-02)

- **No** es un reemplazo de JTAG/GDB. No hay breakpoints de hardware, ni step, ni inspección de memoria arbitraria.
- **No** es un dashboard de producción ni un SCADA. No hay usuarios, ni alertas, ni persistencia a largo plazo.
- **No** es un logger de campo. Las sesiones se graban para *debuggear y compartir un bug*, no para historizar meses.
- **No** compite en ploteo. El plot existe porque a veces la forma de onda del ruido de un sensor es la respuesta, pero no se invierte esfuerzo en hacerlo bonito.
- **No** hay compatibilidad hacia atrás con Mole v1. El protocolo se rediseña de cero.

### 1.4 Catálogo de funcionalidades

Enumeración completa de lo que la herramienta hace. Los códigos `FEAT-xx` son estables y se referencian desde los tickets de implementación; **no son secuenciales dentro de cada tabla**, porque las features nuevas se insertan en su bloque temático conservando su código de origen. La columna **Fase** remite a §16.

#### Logs

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-01 | Logs estructurados | Seis niveles, tag, tarea, core, archivo:línea y timestamp del MCU en cada línea. El contexto viaja como enteros internados, no como texto repetido. | F1 | §7.2 |
| FEAT-02 | Filtrado y búsqueda | Filtros combinables por nivel, tag, tarea, core, texto y regex, resueltos sobre columnas de enteros antes de tocar el string. Objetivo: 10M líneas en <300 ms. | F2 | HOST-09 |
| FEAT-03 | Nivel de log en vivo | Silenciar o elevar un tag desde el desktop sin recompilar. El filtro se aplica en el productor, así que el tag silenciado no consume ancho de banda. | F2 | PR-19 |
| FEAT-04 | Follow / freeze | Seguimiento automático del final del log, con congelamiento al hacer scroll hacia atrás y vuelta al vivo con un atajo. | F2 | UI-02 |
| FEAT-05 | Salto a fuente | Click en una línea de log abre el archivo y la línea en el editor. Con formateo diferido el origen viaja en el `REC_FMT_DEF`: costo cero por record en régimen. | F2 | REC-03 |
| FEAT-50 | Formateo diferido | El MCU manda el id del format string y los argumentos en binario; el host renderiza. Sin `vsnprintf` ni soporte de float de newlib en el firmware. Sintaxis de llaves, verificada en compilación. | F1 | §7.2 |
| FEAT-51 | Reformateo retroactivo | Como los argumentos se guardan tipados, se puede cambiar precisión o base de un valor ya capturado, y filtrar por el valor de un argumento en vez de por texto. | F2 | REC-39 |
| FEAT-06 | Canal raw | Todo lo que llega por el puerto y no es Mole (log del bootloader ROM, un `printf` suelto) se muestra en un canal aparte en vez de descartarse. | F2 | HOST-05 |

#### Variables

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-07 | Watch de variables | Tabla de valores en vivo con tipo, último valor, min/max/media/desvío incrementales e historial en ring. | F2 | §7.3 |
| FEAT-08 | Watch con rate limit | `watchEvery()` descarta en el productor, para instrumentar loops de control de alta frecuencia sin saturar el enlace. | F2 | REC-09 |
| FEAT-09 | Sparkline | Miniatura de la forma de onda en la fila de la tabla, expandible a un panel con gráfico. Deliberadamente secundario. | F2 | REC-08 |
| FEAT-10 | Bind de variables | Ligar una variable del firmware a un control del desktop y editarla en vivo, sin recompilar ni reflashear. Slider, número, toggle, dropdown de enum. | F3 | §7.4 |
| FEAT-11 | Bind con callback | Variante donde cambiar el valor dispara una acción (reconfigurar un periférico, recalcular un coeficiente). | F3 | REC-12 |
| FEAT-12 | Reflejo bidireccional | Si el firmware cambia el valor por código, el control del desktop se actualiza. El control muestra el estado real, no lo último que se tipeó. | F3 | REC-13 |

#### Buffers y memoria

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-52 | Logueo de structs | Un struct se loguea con `{}` como cualquier escalar, con nombres de campo y enums por nombre. Descriptor no intrusivo de una línea, sirve para tipos de terceros. | F1 | §7.2.4 |
| FEAT-53 | Vista de bytes de un struct | `MOLE_DUMP(s)` muestra la memoria real con el padding a la vista, que es donde viven los bugs de alineación. | F4 | REC-45 |
| FEAT-54 | Filtro por campo | Los campos quedan tipados en el store, así que `s.valid == false` o `s.ax > 2.0` son filtros de log válidos, no búsqueda de texto. | F2 | REC-49 |
| FEAT-13 | Dump de buffers | Volcado de un buffer arbitrario con vistas hex, ASCII, binario y decimal, con offsets y dirección de origen. | F4 | §7.5 |
| FEAT-14 | Diff de dumps | Resaltado de los bytes que cambiaron respecto del dump anterior del mismo símbolo. Para probar un sensor I2C es la feature más útil de la herramienta. | F4 | REC-15 |
| FEAT-15 | Pin de referencia | Fijar un dump como referencia y diffear todos los siguientes contra ese, no contra el inmediatamente anterior. | F4 | REC-17 |
| FEAT-16 | Overlay de schema | El firmware declara la estructura del buffer con un mini-DSL y el desktop muestra el hexdump junto a la tabla decodificada, con endianness y bitfields expandidos. | F4 | REC-16 |
| FEAT-17 | Blobs | Transferencia fragmentada de payloads grandes (imagen de cámara, dump de flash, frame de radar) con límite de ancho de banda para no monopolizar el enlace. | F4 | §7.9 |

#### Profiling

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-18 | Spans | Medición de bloques de código con RAII, anidables y con ownership de tarea. Reemplaza los timers planos de v1. | F5 | §7.6 |
| FEAT-19 | Percentiles | Conteo, total, media, p50, p95, p99 y máximo por span. El outlier que rompe el timing no aparece en un promedio. | F5 | REC-19 |
| FEAT-20 | Timeline de spans | Barras anidadas por tarea sobre el eje de tiempo, en canvas. Se ve dónde se solapan las tareas y dónde hay huecos. | F5 | REC-19 |
| FEAT-21 | Árbol de costo | Vista acumulada tipo flamegraph del árbol de spans. | F5 | REC-19 |
| FEAT-22 | Counters | Contadores agregados en el MCU y emitidos como delta cada 250 ms. Seguros desde ISR. Un millón de incrementos cuestan cuatro records por segundo. | F2 | §7.7 |
| FEAT-23 | Eventos | Marcas puntuales con un argumento numérico, para anotar la timeline sin el costo de un log. | F5 | §7.1 |

#### Estado y control de flujo

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-24 | Máquinas de estado | Lanes temporales por máquina con duración de cada estado y transiciones. Para debuggear un join de LoRaWAN o un handshake es la vista correcta. | F5 | §7.8 |
| FEAT-25 | Status LEDs | Indicadores de salud simples (apagado/verde/amarillo/rojo) declarados desde el firmware. | F2 | §7.8 |
| FEAT-26 | Checks | Aserciones que no abortan: un check fallido emite un record urgente con archivo, línea y mensaje, y fuerza flush inmediato. | F4 | §7.10 |
| FEAT-27 | Comandos | Botones y controles declarados en el firmware con argumento tipado. El desktop genera la UI sola, sin configuración. | F3 | §7.11 |
| FEAT-28 | Pausa cooperativa | Detener una tarea en un punto seguro definido por el autor del firmware, sin matar el watchdog y sin `vTaskSuspend()`. Con timeout obligatorio de auto-reanudación. | F6 | §8.6 |
| FEAT-29 | Alcances de pausa | Pausar una tarea, todas, o con barrera de sincronización para obtener un snapshot coherente entre tareas. | F6 | FW-20 |
| FEAT-30 | Step | Reanudar hasta el próximo checkpoint de la tarea. | F6 | PR-18 |
| FEAT-31 | Break on check fail | Pausa automática cuando falla un check, configurable desde el desktop. | F6 | §7.10 |
| FEAT-32 | Editar y seguir | En una tarea pausada, las escrituras de bind y los comandos pendientes se aplican antes de reanudar. | F6 | FW-23 |

#### Integridad del enlace

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-33 | Late-join sync | Abrir la app con el MCU ya andando reconstruye catálogo y último estado conocido en ≤2 s, sin resetear el dispositivo. | F1 | §6.5 |
| FEAT-34 | Contabilidad de pérdida | Todo descarte se cuenta por canal y se marca en la línea de tiempo donde ocurrió. Nunca se pierde nada en silencio. | F1 | §6.6 |
| FEAT-35 | Salud del enlace | Indicador permanente de rec/s, KB/s, drops, huecos de secuencia y RTT. | F2 | PR-14 |
| FEAT-36 | Backpressure configurable | Política por canal (bloquear, descartar nuevo, descartar viejo, diezmar), cambiable en vivo desde el desktop. | F1 | PR-12 |
| FEAT-37 | Timeline global sincronizada | Una barra de tiempo común: hacer scrub mueve todas las vistas al mismo instante. Es lo que convierte varias tablas en un debugger. | F5 | UI-03 |

#### Sesiones

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-38 | Grabación continua | Buffer circular en disco siempre activo. "Guardá los últimos 5 minutos" es un botón, no algo que había que haber activado antes del bug. | F7 | SES-02 |
| FEAT-39 | Replay | Reproducción de una sesión a 1×, a velocidad máxima o paso a paso. El pipeline no distingue replay de hardware real. | F7 | SES-03 |
| FEAT-40 | Compartir un bug | Mandás el `.molecap` y el otro ve exactamente lo mismo. También habilita desarrollar el desktop sin hardware. | F7 | SES-04 |
| FEAT-41 | Exportación | Logs a texto/JSONL, watches y spans a CSV/Parquet, dumps a binario. | F7 | SES-05 |

#### Experiencia de desarrollo

| Código | Feature | Descripción | Fase | Ref |
|---|---|---|---|---|
| FEAT-42 | Flash guard | Detecta que se va a flashear, libera el puerto y vuelve solo cuando termina. Sin "Port busy". | F7 | DX-01 |
| FEAT-43 | Reconexión automática | Backoff con retención del catálogo. Un reset del MCU no pierde el historial; un firmware distinto marca una división visible en la timeline. | F7 | DX-02 |
| FEAT-44 | Autodetección de puerto | Preselección por VID/PID de Espressif y de puentes USB-serial conocidos, con baudrate coherente con la librería. | F7 | DX-03 |
| FEAT-45 | Modo demo | Firmware simulado embebido que genera tráfico representativo, para evaluar la herramienta sin hardware. | F7 | DX-04 |
| FEAT-46 | Snippets copiables | Cada símbolo ofrece copiar la línea de C++ que lo produce. | F7 | DX-05 |
| FEAT-47 | Coste cero en release | `MOLE_ENABLED=0` compila toda la instrumentación a nada, sin dejar strings ni símbolos en el binario. La instrumentación se queda en el código. | F1 | FW-12 |
| FEAT-48 | `molectl` headless | El pipeline completo sin UI: benchmark, captura, CI. | F1 | ARQ-03 |
| FEAT-55 | Workspace con splitters | Árbol de paneles arrastrables con pestañas, desprendibles a ventana propia y guardables como preset con nombre. | F2 | UI-04 |
| FEAT-56 | Presets de layout | Disposiciones de fábrica para sensor, timing, protocolo y control, conmutables por atajo. | F2 | UI-06 |
| FEAT-57 | Modos de tiempo en el log | Absoluto, relativo a la sesión, delta con la fila anterior o con una marca. Convierte el log en instrumento de medición. | F2 | UI-13 |
| FEAT-58 | Resaltado por reglas | Pintar líneas por patrón sin ocultar el resto: filtrar da las líneas, resaltar da las líneas y su contexto. | F2 | UI-16 |
| FEAT-59 | Marcadores | Marcar filas, navegar entre marcas y usarlas como origen del delta de tiempo. | F2 | UI-18 |
| FEAT-60 | Agrupación jerárquica | Los símbolos con punto (`imu.ax`) se agrupan en árbol colapsable en el panel de watch. | F2 | UI-28 |
| FEAT-61 | Estado sucio en binds | El control marca cuándo el valor local difiere del confirmado por el dispositivo, con revertir. | F3 | UI-29 |
| FEAT-62 | Paleta de comandos | `Cmd+K` para todo: presets, paneles, filtros, comandos del firmware, marcadores. | F2 | UI-38 |
| FEAT-49 | Transporte Wi-Fi | Mismo protocolo sobre TCP con descubrimiento mDNS y token opcional, para dispositivos ya desplegados. | F8 | TR-04 |

---

## 2. Plataformas soportadas

### 2.1 Firmware (PLAT-01)

| Familia | Soporte | Transporte preferido |
|---|---|---|
| ESP32 (original) | Completo | UART |
| ESP32-S2 | Completo | USB-CDC nativo |
| ESP32-S3 | Completo (target de referencia) | USB-CDC nativo |
| ESP32-C3 / C6 | Completo | USB-Serial-JTAG |
| ESP32-H2 | Completo | USB-Serial-JTAG |
| ESP8266 | **Sin soporte** | — |
| AVR / Arduino Uno | **Sin soporte** | — |

**PLAT-02** — Se descartan AVR y ESP8266 de forma explícita y definitiva. La v1 los declaraba y ni siquiera compilaba en AVR. Todo el diseño asume: 32 bits, FreeRTOS presente, `esp_timer_get_time()` de 64 bits, ≥160 MHz, y al menos 30 KB de RAM libres.

**PLAT-03** — Frameworks soportados: ESP-IDF (nativo) y Arduino-ESP32 (que corre sobre IDF). La librería DEBE compilar en ambos con el mismo código, usando APIs de IDF directamente y no de Arduino.

**PLAT-05** — **No existe capa de abstracción de plataforma.** La librería llama a ESP-IDF directamente. No se define un HAL, ni un `IMolePlatform`, ni wrappers sobre `esp_timer`, `esp_rom_crc32_le`, `esp_task_wdt_*` o TinyUSB. Portar a RP2040 o STM32 no es un objetivo de v2 y abstraer preventivamente cuesta complejidad hoy contra un beneficio hipotético. La única abstracción que sí existe es la de transporte (TR-07), porque hay tres transportes reales desde el día uno. Resuelto en PA-02.

### 2.2 Desktop (PLAT-04)

macOS (target de referencia), Linux y Windows. Rust + Tauri 2; el framework de UI se decide en F2 (PA-12, ver §10.9), con librería de componentes preferentemente headless (UI-44).

---

## 3. Arquitectura general

```
   FIRMWARE (ESP32)                         DESKTOP
┌──────────────────────┐            ┌────────────────────────────┐
│ tarea app 1 ─┐       │            │  transporte (hilo)         │
│ tarea app 2 ─┼→ ring │            │        ↓ bytes crudos      │
│ tarea app N ─┘  buf  │            │  decoder (hilo)            │
│ ISR ─────────→  (SPSC│            │        ↓ records           │
│                 x N) │            │  store columnar (Rust)     │
│                  ↓   │  frames    │        ↓                   │
│            [moleTask]├───────────→│  coalescer @30Hz           │
│                  ↑   │            │        ↓ snapshot compacto │
│            control   │←───────────┤  IPC Tauri                 │
│            (EventGrp)│  downlink  │        ↓                   │
└──────────────────────┘            │  UI Vue (virtualizada)     │
                                    └────────────────────────────┘
```

**ARQ-01** — La `moleTask` es la **única dueña del transporte**. Ninguna tarea de usuario escribe al puerto. Esto elimina la corrupción de frames de v1 (buffers compartidos en la instancia global) y habilita batching y pausa.

**ARQ-02** — El desktop mantiene **todo el estado en Rust**. La UI es una vista. No existe un array de mensajes en JavaScript que crezca sin límite. Esta es la decisión de la que depende todo el presupuesto de performance de §4.

**ARQ-03** — El pipeline del host DEBE poder correr sin UI (binario `molectl`). Sirve para benchmark, para CI, y para capturar sesiones desde un headless.

---

## 4. Presupuestos de performance

Criterios de aceptación medibles. Cada uno tiene un test asociado en §15.

| Código | Métrica | Objetivo | Medición |
|---|---|---|---|
| PERF-01 | Costo de un log con dos argumentos escalares, cola no llena | < 1 µs @240 MHz | ciclos de CPU, S3 |
| PERF-02 | Costo de `mole.watch()` en la tarea de usuario | < 1 µs | idem |
| PERF-03 | Jitter agregado a una tarea de control periódica con Mole activo a 20k rec/s | < 50 µs p99 | GPIO + osciloscopio |
| PERF-04 | Throughput sostenido, USB-CDC nativo (S3) | ≥ 600 KB/s | `molectl bench` |
| PERF-05 | Throughput sostenido, UART @2 Mbaud | ≥ 180 KB/s | idem |
| PERF-06 | Records/s sostenidos end-to-end sin pérdida (USB-CDC) | ≥ 50.000 rec/s | idem |
| PERF-07 | Ráfaga absorbida sin pérdida (buffer de MCU) | ≥ 8.000 records | idem |
| PERF-08 | Latencia MCU→pantalla, p95 | < 120 ms | timestamp roundtrip |
| PERF-09 | UI a 60 fps con ingesta de 50k rec/s | sin frames caídos | devtools |
| PERF-10 | RAM del host con 10M records en store | < 1,5 GB | RSS |
| PERF-11 | Filtrado de 10M líneas de log por tag+texto | < 300 ms | benchmark |
| PERF-12 | Footprint RAM en MCU, configuración por defecto | < 24 KB | `heap_caps_get_info` |
| PERF-14 | Costo de loguear un struct de 6 campos | < 2 µs | ciclos de CPU, S3 |
| PERF-15 | Flash ahorrado por no linkear el soporte de float de `printf` | ≥ 4 KB | `size` del ELF, con y sin |

**PERF-13** — Si un objetivo no se alcanza, se ajusta el objetivo **explícitamente en este documento con justificación**, no se ignora.

---

## 5. Transporte

### 5.1 Transportes definidos

**TR-01 — UART.** Todas las familias. Baudrates soportados hasta 2.000.000. Default sugerido 921600. Requiere que el puente USB-serial lo banque (CP2102: 3M, CH340: 2M, FTDI: 3M).

**TR-02 — USB-CDC nativo (TinyUSB).** S2/S3. **Transporte preferido.** Sin baudrate (el parámetro se ignora). Es el que cumple PERF-04.

**TR-03 — USB-Serial-JTAG.** C3/C6/H2. Periférico dedicado, sin puente externo, ~300 KB/s prácticos.

**TR-04 — TCP sobre Wi-Fi.** Mismo framing, puerto 3141/tcp. Para dispositivos ya desplegados donde el USB no es accesible. El MCU corre como servidor, una sola conexión activa. Descubrimiento por mDNS (`_mole._tcp`).

**TR-05 — USB vendor bulk (diferido).** Endpoints bulk crudos vía TinyUSB, driver de host con `nusb`. Evita el overhead de line-coding de CDC-ACM. Es la ruta si PERF-04 resulta insuficiente. **Ver PA-03.**

### 5.2 Reglas

**TR-06** — El framing (§6) DEBE ser idéntico en todos los transportes. Un `.molecap` grabado por UART DEBE poder reproducirse como si viniera de TCP.

**TR-07** — El transporte es un trait en Rust (`Transport: Read + Write + Send`) y una interfaz abstracta en C++ (`IMoleTransport`). Agregar TR-05 no DEBE tocar el decoder ni el store.

**TR-08** — En TR-02/TR-03, el firmware DEBE detectar si el host está conectado (`tud_cdc_connected()` o equivalente). Sin host conectado, los records se descartan en el productor sin costo, y el ring buffer no se llena.

**TR-09** — El desktop DEBE reconectar automáticamente con backoff (100 ms → 5 s) cuando el puerto desaparece. En USB nativo, un reset del MCU hace desaparecer y reaparecer el device: esto es el flujo normal, no un error.

---

## 6. Framing y protocolo

> **CONGELADO en draft.8.** Todo cambio a §6 o §7 a partir de acá requiere versión menor nueva y actualización de los vectores de TEST-01.

### 6.1 Capas

```
[ record ][ record ][ record ] ...     ← capa de datos (§7)
└──────────── frame ─────────────┘     ← capa de agregación (§6.2)
└──── COBS(frame) ────┘ 0x00           ← capa de framing
```

**PR-01** — El framing es COBS con delimitador `0x00`. Overhead peor caso 1/254. Se mantiene de v1 porque es correcto y barato; lo que cambia es todo lo de arriba.

### 6.2 Frame

**PR-02** — Un frame agrupa N records. **Este es el cambio de performance más importante respecto de v1**, donde cada dato era un frame con su propio CRC y delimitador. Agrupar amortiza el costo de framing sobre decenas de records.

Layout del frame (antes de COBS, little-endian):

| Offset | Tipo | Campo | Notas |
|---|---|---|---|
| 0 | u8 | `ver_flags` | bits 0-3: versión de protocolo (=2). bits 4-7: flags |
| 1 | u16 | `seq` | secuencia de frame, wrap en 65535 |
| 3 | u64 | `t_base_us` | `esp_timer_get_time()` al abrir el frame |
| 11 | u16 | `rec_count` | cantidad de records |
| 13 | … | records | concatenados |
| −4 | u32 | `crc32` | CRC32 (poly zlib) sobre bytes `[0 .. len-5]` |

Flags del frame:

| Bit | Nombre | Significado |
|---|---|---|
| 4 | `FLAG_CATALOG` | El frame contiene definiciones de catálogo |
| 5 | `FLAG_DROPS` | Hubo descartes antes de este frame (ver `REC_STATS`) |
| 6 | `FLAG_PAUSED` | El MCU está en estado pausado |
| 7 | reservado | — |

**PR-03** — CRC32 en lugar de CRC8. Con frames de hasta 4 KB, un CRC8 no protege nada. En ESP32 se usa `esp_rom_crc32_le()`, que está en ROM y es más rápido que la implementación bit a bit de v1.

**PR-04** — Tamaño máximo de frame: 4096 bytes pre-COBS. Configurable en compilación (`MOLE_FRAME_MAX`), mínimo 512.

**PR-05** — La `moleTask` cierra y emite un frame cuando ocurre lo primero de: (a) el frame llega a `MOLE_FRAME_MAX`, (b) pasaron `MOLE_FLUSH_MS` (default 5 ms) desde el primer record, (c) llega un record con flag de urgencia (`REC_CHECK_FAIL`, `REC_PAUSED`), (d) `mole.flush()` explícito.

**PR-06** — El host DEBE detectar huecos en `seq` y reportarlos como pérdida de transporte, distinta de la pérdida por backpressure del MCU (§6.6).

### 6.3 Record

**PR-07** — Header de record, 4 bytes:

| Offset | Tipo | Campo | Notas |
|---|---|---|---|
| 0 | u8 | `type` | tipo de record (§7) |
| 1 | u8 | `len` | longitud del payload, 0–255 |
| 2 | u16 | `dt_us` | µs transcurridos desde `t_base_us` del frame |

**PR-08** — `dt_us` cubre 65,5 ms. Si un record llega con delta mayor (frame abierto por mucho tiempo con poco tráfico), la `moleTask` DEBE cerrar el frame y abrir uno nuevo. Esto es automático por PR-05(b) con `MOLE_FLUSH_MS` ≤ 65.

**PR-09** — Payloads > 255 bytes usan `REC_BLOB_*` con fragmentación explícita (§7.9). Ningún record excede 255 bytes de payload.

**PR-10** — **Todo record lleva timestamp del MCU.** En v1 el desktop timestampeaba a la llegada, lo que metía el jitter del buffer serial dentro de los datos. Para un profiler eso es descalificante.

### 6.4 Catálogo de símbolos

**CAT-01** — Se elimina el hash FNV-16 de v1. Los símbolos se identifican con un **`sym_id: u16` secuencial** asignado en orden de registro. Cero colisiones, por construcción.

**CAT-02** — El hot path no hashea ni compara strings. El patrón de uso es:

```cpp
// El id se resuelve una sola vez, la primera vez que se ejecuta la línea.
mole.watch("Voltage", v);     // azúcar: expande a un static local con el id
```

expandido por macro a:

```cpp
do { static mole::SymId _s = mole::intern("Voltage", mole::SymKind::Watch);
     mole::watch_by_id(_s, v); } while(0)
```

El costo en régimen es un `load` de una variable estática. Para uso en templates o cuando se quiere el id explícito, `mole::Sym s("Voltage");` como global.

**CAT-03** — Tipos de símbolo (`SymKind`): `Watch`, `Tag`, `Task`, `File`, `Command`, `Span`, `Counter`, `Machine`, `State`, `Bind`, `Dump`, `Schema`.

**CAT-04** — `REC_SYM_DEF` payload: `{ sym_id: u16, kind: u8, parent: u16, name: char[] }`. `parent` liga, por ejemplo, un `State` a su `Machine`, o un símbolo a su `File`.

**CAT-05** — Límite por defecto: 512 símbolos (`MOLE_MAX_SYMBOLS`). Al desbordar, `intern()` devuelve `SYM_OVERFLOW` (0xFFFF) y se emite **un solo** `REC_STATS` con el contador de overflow. **Nunca** se reintenta registrar en loop (bug de v1: registry lleno ⇒ reenvío de la definición en cada llamada, inundando el enlace).

**CAT-06** — `sym_id = 0` significa "sin símbolo" / nulo.

### 6.5 Sesión, epoch y resync

**CAT-07** — Al arrancar, el firmware genera un `epoch: u32` (contador en RTC memory, o aleatorio si no está disponible). El `epoch` identifica unívocamente un arranque.

**CAT-08** — El firmware mantiene un `catalog_hash: u32` incremental sobre todas las definiciones emitidas.

**CAT-09** — Handshake:

```
Desktop → MCU:  CTL_HELLO { proto_ver: u8, known_epoch: u32, known_catalog_hash: u32 }
MCU → Desktop:  REC_SESSION { epoch, catalog_hash, chip_model, chip_rev, idf_ver[],
                              app_name[], app_build_time[], app_elf_sha256[8],
                              cpu_freq_mhz, free_heap, mole_ver }
```

**CAT-10** — Si `known_epoch != epoch` o `known_catalog_hash != catalog_hash`, el MCU DEBE re-emitir el catálogo completo más el **último valor conocido** de cada watch, bind, status y máquina de estados. Esto es el *late-join sync* que v1 prometía en el README y no tenía implementado en ninguna de las dos puntas.

**CAT-11** — El MCU DEBE emitir un `REC_SESSION` no solicitado cada 2 s mientras no haya recibido un `CTL_HELLO`. Así, abrir la app con el MCU ya andando converge en ≤2 s sin intervención.

**CAT-12** — `app_elf_sha256[8]` (8 bytes del hash del ELF) permite al desktop advertir "el firmware cambió" y decidir si continúa la sesión o la divide (DX-02). El uso del ELF para resolver símbolos, DWARF y backtraces queda **diferido a v3** (PA-07, §19); el campo se incluye igual en v2 porque cuesta 8 bytes una vez por sesión y es el anclaje que v3 va a necesitar.

### 6.6 Pérdida y contabilidad

**PR-11** — Un debugger que pierde datos en silencio es peor que uno que no anda. **Toda pérdida DEBE ser contabilizada y visible.**

**PR-12** — Política de backpressure configurable por canal:

| Política | Comportamiento |
|---|---|
| `Block` | El productor espera lugar en el ring. Datos exactos, timing alterado. |
| `DropNewest` | Descarta el record nuevo. Default para `Log` y `Watch`. |
| `DropOldest` | Descarta el más viejo del ring. Default para `Watch` en modo "último valor". |
| `Decimate` | Descarta 1 de cada N crecientemente bajo presión. Opción para `Watch` de alta frecuencia. |

**PR-13** — `REC_STATS` se emite cada 500 ms y ante cada cambio de `dropped`: `{ enqueued: u32, dropped: u32, dropped_by_kind: u16[8], ring_high_water: u16, sym_overflow: u16, tx_bytes: u32, free_heap: u32, min_free_heap: u32 }`.

**PR-14** — La UI DEBE mostrar un indicador permanente de salud del enlace: rec/s, KB/s, drops acumulados, huecos de `seq`. Si hubo drops, DEBE marcarse **en la línea de tiempo del log** el punto donde ocurrieron, no solo en un contador global.

### 6.7 Downlink

**PR-15** — El downlink usa **exactamente el mismo framing** que el uplink: COBS + frame + records + CRC32. Se elimina la asimetría de v1 (bytes crudos para control, COBS a medias para comandos), que era la causa raíz del bug del `param` corrupto y del reset espurio por ruido.

**PR-16** — El firmware DEBE verificar el CRC32 de cada frame de downlink y descartar silenciosamente los inválidos. Un byte de ruido no puede reiniciar el MCU.

**PR-17** — El parseo de downlink ocurre en la `moleTask`, nunca en una ISR. Los efectos (ejecutar comando, escribir un bind) se aplican en un punto seguro definido en §8.6.

**PR-18** — Records de downlink:

| Tipo | Payload | Efecto |
|---|---|---|
| `CTL_HELLO` | ver §6.5 | Handshake / resync |
| `CTL_CMD` | `{ cmd_id: u16, arg_type: u8, arg[] }` | Invoca comando registrado |
| `CTL_BIND_SET` | `{ sym_id: u16, type: u8, value[] }` | Escribe variable ligada |
| `CTL_PAUSE` | `{ scope: u8, task_id: u8 }` | Solicita pausa (§8.6) |
| `CTL_RESUME` | `{ scope: u8, task_id: u8 }` | Reanuda |
| `CTL_STEP` | `{ task_id: u8 }` | Reanuda hasta el próximo checkpoint |
| `CTL_RESET` | `{ magic: u32 = 0x4D4F4C45 }` | Reinicia. **Requiere magic**, no un byte suelto. |
| `CTL_SET_LEVEL` | `{ sym_id: u16, level: u8 }` | Nivel de log por tag, en vivo |
| `CTL_SET_POLICY` | `{ kind: u8, policy: u8 }` | Cambia backpressure en vivo |
| `CTL_PING` | `{ nonce: u32 }` | Mide RTT; responde `REC_PONG` |

**PR-19** — `CTL_SET_LEVEL` permite silenciar un tag ruidoso **sin recompilar y sin gastar ancho de banda**: el filtro se aplica en el productor, antes de encolar. Es el mecanismo más efectivo para sostener PERF-06 en firmwares reales.

---

## 7. Modelo de datos: canales y records

### 7.1 Tabla de tipos

| Código | Tipo | Canal | Payload |
|---|---|---|---|
| 0x01 | `REC_SESSION` | meta | §6.5 |
| 0x02 | `REC_SYM_DEF` | meta | §6.4 |
| 0x03 | `REC_STATS` | meta | §6.6 |
| 0x04 | `REC_PONG` | meta | `{ nonce: u32 }` |
| 0x05 | `REC_FMT_DEF` | meta | §7.2 |
| 0x06 | `REC_TYPE_DEF` | meta | §7.2.4 |
| 0x10 | `REC_LOG` | log | §7.2 — formateado en MCU (fallback) |
| 0x11 | `REC_LOG_FMT` | log | §7.2 — formateo diferido (default) |
| 0x20 | `REC_WATCH` | watch | §7.3 |
| 0x21 | `REC_WATCH_STR` | watch | `{ sym: u16, s: char[] }` |
| 0x30 | `REC_BIND_DEF` | bind | §7.4 |
| 0x31 | `REC_BIND_VAL` | bind | `{ sym: u16, type: u8, value[] }` |
| 0x40 | `REC_DUMP` | dump | §7.5 |
| 0x41 | `REC_SCHEMA_DEF` | dump | §7.5 |
| 0x50 | `REC_SPAN_BEGIN` | span | §7.6 |
| 0x51 | `REC_SPAN_END` | span | §7.6 |
| 0x52 | `REC_SPAN_ABORT` | span | `{ span_id: u16, reason: u8 }` |
| 0x60 | `REC_COUNTER` | counter | §7.7 |
| 0x70 | `REC_STATE` | fsm | §7.8 |
| 0x71 | `REC_STATUS` | fsm | `{ sym: u16, level: u8 }` |
| 0x80 | `REC_CHECK_FAIL` | check | §7.10 |
| 0x81 | `REC_EVENT` | event | `{ sym: u16, arg: u32 }` |
| 0x90 | `REC_CMD_DEF` | cmd | §7.11 |
| 0xA0 | `REC_BLOB_BEGIN` | blob | §7.9 |
| 0xA1 | `REC_BLOB_CHUNK` | blob | §7.9 |
| 0xF0 | `REC_PAUSED` | ctrl | `{ task_id: u8, sym: u16, line: u16, reason: u8 }` |
| 0xF1 | `REC_RESUMED` | ctrl | `{ task_id: u8 }` |

### 7.2 Logs (REC-01)

**El formateo es diferido al host por defecto.** El MCU no corre `vsnprintf`: emite el identificador del format string más los argumentos en binario. Resuelto en PA-01.

#### 7.2.1 Sintaxis

**REC-32** — Los format strings usan **llaves** (`{}`, `{:.2}`, `{:04X}`), no `printf`. El string lleva únicamente *presentación* (precisión, base, ancho); el **tipo lo aporta C++**, nunca el string. Esto elimina por construcción la clase de bug del `%d` con un float, y mapea casi uno a uno con el formato nativo de Rust del lado del host.

```cpp
MOLE_INFO("ADC crudo={} volts={:.2}", raw, volts);
MOLE_WARN(net, "reintento {}/{} rssi={}", n, MAX_RETRIES, rssi);
MOLE_ERROR("CRC malo: esperado={:04X} recibido={:04X}", exp, got);
```

**REC-33** — El format string DEBE ser un literal. La cantidad de `{}` se verifica contra `sizeof...(Args)` en compilación con `static_assert`. No existe formato dinámico.

#### 7.2.2 Records

**REC-34** — `REC_FMT_DEF` se emite una sola vez por sitio de log, la primera vez que la línea se ejecuta:

`{ fmt_id: u16, file_sym: u16, line: u16, argc: u8, arg_types: u8[argc], fmt: char[] }`

**REC-35** — `REC_LOG_FMT` es el record del camino normal:

`{ level: u8, task_id: u8, core: u8, tag_sym: u16, fmt_id: u16, args[] }`

Los argumentos van empaquetados en orden, sin etiquetas de tipo: los tipos ya viajaron en el `REC_FMT_DEF`. El host resuelve el layout mirando `arg_types`.

**REC-36** — `REC_LOG` (formateado en el MCU) se mantiene como **fallback explícito** para el caso en que el mensaje se arma en runtime:

`{ level: u8, task_id: u8, core: u8, tag_sym: u16, file_sym: u16, line: u16, msg: char[] }`

Se accede por `mole::logs()`, nunca por las macros. Los dos records conviven de forma permanente.

#### 7.2.3 Argumentos

**REC-37** — Tipos soportados: enteros de 8/16/32/64 bits con y sin signo, `bool`, `float`, `double`, punteros, literales de cadena y cadenas de runtime. Un tipo no soportado es un **error de compilación**, no una conversión silenciosa.

**REC-38** — Cadenas, los dos casos:

| Caso | Tratamiento | Costo en el wire |
|---|---|---|
| Literal (`const char(&)[N]`) | Se interna como símbolo | 2 bytes, para siempre |
| Runtime (`const char*`, `string_view`) | Copia inline | 1 + len bytes |

Es la única situación en la que el camino caliente hace una copia. Se detecta por sobrecarga, no por heurística.

#### 7.2.4 Logueo de structs

**REC-40** — Un struct es un argumento de log legal si tiene **descriptor**. El descriptor es una macro **no intrusiva**, escrita al lado del struct y no adentro, de modo que funciona igual con tipos de una librería de terceros o de un SDK que no se puede tocar.

```cpp
struct SensorSample {
  float    ax, ay, az;
  uint16_t t_ms;
  Mode     mode;
  bool     valid;
};

MOLE_DESCRIBE_ENUM(Mode, RAW, AVG, MEDIAN);
MOLE_DESCRIBE(SensorSample, ax, ay, az, t_ms, mode, valid);
```

```cpp
MOLE_INFO("muestra {}", s);
// muestra SensorSample{ax=0.12, ay=-9.79, az=0.03, t_ms=1204, mode=AVG, valid=true}

MOLE_WARN(imu, "fuera de rango {:#} umbral={:.1}", s, thr);
// {:#} → renderizado expandido como tabla en el panel de log
```

**REC-41** — Los enums descriptos se renderizan por nombre (`AVG`), no por valor (`1`). Un enum sin descriptor se renderiza como entero.

**REC-42** — Implementación: `MOLE_DESCRIBE` genera una función libre `constexpr` hallada por ADL, que devuelve un `TypeDesc` con nombre, tipo wire, `offsetof` y `sizeof` de cada campo. Con eso `wire_type_v<T>` resuelve a `Struct` y el tipo entra al sistema de argumentos como cualquier `int`. No hay herencia, ni virtuales, ni registro global en tiempo de arranque.

**REC-43** — Registro perezoso: la primera ejecución del sitio de log emite `REC_TYPE_DEF` y no se repite.

`{ type_id: u16, name: char[], nfields: u8, fields: [{ name_sym: u16, wire: u8, flags: u8, offset: u16, size: u16, ref_type: u16 }] }`

`flags` por campo: bit 0 = **big-endian** (lo marcan `MOLE_DESCRIBE_BE` y el sufijo `be` del DSL, REC-50); bits 1–7 reservados, DEBEN emitirse en cero y el host DEBE ignorarlos.

**REC-44** — En `REC_FMT_DEF`, el tipo de argumento `0xF0 = Struct` va seguido inline de los 2 bytes del `type_id`. En `REC_LOG_FMT` viajan **solo los valores de los campos**, empaquetados. El costo en el wire es idéntico a loguear los campos sueltos, menos los `{}` del format string.

**REC-45** — **Dos modos de empaquetado, con semántica distinta:**

| | Fiel a los valores | Fiel a los bytes |
|---|---|---|
| Empaquetado | Secuencial, sin padding | `memcpy` crudo, con padding |
| Sirve para | Ver el estado lógico | Ver la memoria real |
| Soporta | Anidados, cadenas, punteros | Solo POD plano |
| Entrada | `MOLE_INFO("{}", s)` | `MOLE_DUMP(s)` |

El modo bytes **muestra el padding explícitamente**, porque el padding es justamente donde aparecen los bugs de alineación. Un solo `MOLE_DESCRIBE` alimenta los dos modos: valores usa el orden de los campos, bytes usa el `offsetof` que el descriptor ya guarda.

Campos big-endian (flag bit 0, REC-43): **el MCU nunca invierte bytes**. En los dos modos empaqueta los bytes tal como están en memoria; el host invierte al decodificar. Costo cero en el MCU, que es donde importa.

**REC-46** — Bordes definidos:

| Caso | Tratamiento |
|---|---|
| Struct anidado | El campo referencia otro `type_id`; resolución recursiva, profundidad máxima 4 |
| Arreglo (`float ax[3]`) | Requiere `MOLE_FIELD_ARRAY`; `offsetof`+`sizeof` no distinguen arreglo de blob |
| `const char*` como campo | El host decodifica **secuencialmente, no por offset**, para todo el struct |
| Puntero | Se renderiza como dirección hex. Resolución a símbolo queda para v3 (V3-01) |
| Empaquetado > 255 bytes | Se promueve a blob (§7.9) o se trunca con flag. Nunca en silencio |

**REC-47** — El filtro `enabled(lvl, tag)` corre **antes** de empaquetar. Un struct grande en un tag silenciado cuesta cero. Es la razón por la que FEAT-03 importa más con structs que con escalares.

**REC-48** — **Escotilla de escape sin descriptor:**

```cpp
MOLE_INFO("crudo {}", MOLE_BYTES(s));   // hexdump inline, sin descriptor
```

Salida degradada, cero configuración. DEBE existir: si describir fuera obligatorio, el usuario vuelve a concatenar strings a mano y se pierde todo lo demás.

**REC-49** — Como los campos quedan guardados **tipados** en el store del host (REC-39), el filtrado de logs deja de ser textual: `s.valid == false` o `s.ax > 2.0` son filtros legítimos sobre columnas. Y la fila del log se muestra compacta y se expande a tabla con un click, porque el host tiene la estructura y no un string ya cocinado.

**REC-02** — Niveles: `TRACE(0)`, `DEBUG(1)`, `INFO(2)`, `WARN(3)`, `ERROR(4)`, `FATAL(5)`.

**REC-03** — `file_sym` + `line` se capturan con `__FILE__`/`__LINE__` vía macro y se internan una sola vez. Con formateo diferido viajan dentro del `REC_FMT_DEF`, así que en régimen su costo en el wire es **cero**. Configurable con `MOLE_LOG_SOURCE` (default on en debug, off en release).

**REC-04** — Motivación de la decisión. Para `MOLE_INFO("ADC crudo={} volts={:.2}", raw, volts)`:

| | Bytes en el wire | CPU en el MCU |
|---|---|---|
| Formateado en MCU | ~39 | `vsnprintf` con float |
| Diferido | 19 | ~10 stores |

El ahorro de bytes es secundario. Lo que decide es el **float**: `vsnprintf` con `%f` en ESP32 cuesta decenas de microsegundos y linkea el soporte de punto flotante de newlib (varios KB de flash). Para el caso de uso central —prototipar sensores, donde todo es `float`— sacar el formateo de floats del MCU pesa más que ahorrar 20 bytes por línea.

**REC-05** — Longitud máxima del mensaje formateado (fallback `REC_LOG`): 200 bytes. Los mensajes más largos se truncan y el truncado se marca con un flag, no en silencio.

**REC-39** — **El formateo se vuelve retroactivo.** Como el `.molecap` guarda `fmt_id` + argumentos crudos, el host puede reformatear a posteriori: cambiar la precisión de un float, ver un entero en hexadecimal, o filtrar por *el valor del argumento 2* en vez de por texto. Con el string ya formateado en el MCU eso se pierde para siempre. El store del host DEBE conservar los argumentos tipados, no solo la línea renderizada.

### 7.3 Watch (REC-06)

Payload: `{ sym: u16, type: u8, value[] }` donde `type` ∈ `{i8,u8,i16,u16,i32,u32,i64,u64,f32,f64,bool}`.

**REC-07** — El desktop mantiene por watch: último valor, historial en ring configurable (default 10.000 muestras), min/max/media/desvío **calculados de forma incremental correcta** (el promedio de v1 se rompía después de 50 muestras porque el contador quedaba fijado al tope del historial).

**REC-08** — El plot es un sparkline en la fila de la tabla y un panel expandible. No es la vista principal. Ver OBJ-02.

**REC-09** — `mole.watch()` DEBERÍA soportar un modo *rate-limited* por símbolo (`mole.watchEvery("v", val, 50)`), que descarta en el productor. Llamar a `watch` dentro de un loop de control a 10 kHz es el patrón esperado, y el filtro tiene que estar antes de la cola.

### 7.4 Bind — variables editables en vivo (REC-10)

**El diferencial más fuerte para prototipado.** Ligar una variable del firmware a un control del desktop, editable sin recompilar.

```cpp
static float threshold = 2.5f;
static int   samples   = 16;
static bool  use_filter = true;

void setup() {
  mole.bind("threshold", &threshold, 0.0f, 5.0f, 0.01f);
  mole.bind("samples",   &samples,   1, 256);
  mole.bind("use_filter", &use_filter);
  mole.bind("mode", &mode, {"RAW", "AVG", "MEDIAN"});   // enum → dropdown
}
```

`REC_BIND_DEF`: `{ sym: u16, type: u8, flags: u8, min[], max[], step[] }`.

**REC-11** — La escritura desde el desktop (`CTL_BIND_SET`) se aplica en el punto seguro (§8.6), no desde el contexto de recepción. Para tipos ≤4 bytes con alineación natural la escritura es atómica; para tipos mayores DEBE usarse el mutex del bind o un callback.

**REC-12** — `mole.bind()` PUEDE recibir un callback en vez de un puntero, para casos donde cambiar el valor requiere acción (reconfigurar un periférico):

```cpp
mole.bind("tx_power", &tx_power, 2, 20, 1, [](){ radio.setTxPower(tx_power); });
```

**REC-13** — El firmware DEBE emitir `REC_BIND_VAL` cuando el valor cambia por código, para que el control del desktop refleje el estado real y no solo lo que el usuario tipeó.

### 7.5 Dump de buffers con schema (REC-14)

**El segundo diferencial.** Probar un sensor nuevo es, casi siempre, mirar registros.

```cpp
uint8_t regs[14];
i2c.readRegs(0x3B, regs, 14);
mole.dump("mpu_regs", regs, 14);
```

`REC_DUMP`: `{ sym: u16, schema_sym: u16, flags: u8, addr: u32, data[] }`

**REC-15** — El desktop renderiza: hex + ASCII + offsets, y **resalta los bytes que cambiaron respecto del dump anterior del mismo símbolo**. Esta sola feature vale más, para debuggear un sensor I2C, que todo el módulo de ploteo.

**REC-16** — Overlay de estructura. La **fuente única de verdad es el descriptor** de §7.2.4: el mismo `MOLE_DESCRIBE` que habilita loguear el struct habilita decodificar un dump con ese layout. No se escribe el layout dos veces.

```cpp
struct MpuAccel { int16_t ax, ay, az, temp, gx, gy, gz; };   // campos big-endian
MOLE_DESCRIBE_BE(MpuAccel, ax, ay, az, temp, gx, gy, gz);

uint8_t regs[14];
i2c.readRegs(0x3B, regs, 14);
mole::dump("mpu_regs", regs, sizeof(regs), MOLE_TYPE(MpuAccel));
```

El desktop muestra el hexdump *y* la tabla decodificada al lado, resolviendo el layout desde el `REC_TYPE_DEF` que ya conoce. `MOLE_DESCRIBE_BE` marca los campos como big-endian, que es el caso normal al leer registros de un sensor.

**REC-50** — El mini-DSL de texto se mantiene como **camino secundario**, para layouts que no tienen un struct de C++ detrás (un formato de trama de un protocolo ajeno, un bloque de flash, un dump de otro dispositivo):

```cpp
mole::schema("lora_hdr", "u8 ver; u16 src; u16 dst; u8 flags:3; u8 hops:5; u16 crc;");
```

```
campo    := tipo ident [ "[" n "]" ] [ ":" bits ] ";"
tipo     := u8|i8|u16|i16|u32|i32|u64|i64|f32|f64|char   (sufijo "be" para big-endian)
```

Ambos caminos convergen en el mismo `REC_TYPE_DEF`: el DSL se parsea en el host y produce la misma estructura que genera la macro. El renderizador es uno solo.

**REC-51** — Bitfields (`u8 flags:3;`) se decodifican y muestran expandidos. Disponibles solo por el DSL: `offsetof` no alcanza para describir un bitfield de C++, y su layout no está garantizado por el estándar.

**REC-17** — Modos de vista en el desktop: Hex, ASCII, Binario, Decimal, Overlay de schema, y **Diff contra un dump fijado** (pin de referencia).

### 7.6 Spans — profiling (REC-18)

Reemplaza los timers de v1. Anidables, con ownership de tarea.

```cpp
void radioTask(void*) {
  for (;;) {
    MOLE_SPAN("tx_cycle");                 // RAII, cierra al salir de scope
    {
      MOLE_SPAN("build_frame");
      buildFrame();
    }
    {
      MOLE_SPAN("radio_tx");
      radio.transmit(buf, len);
    }
    vTaskDelay(pdMS_TO_TICKS(100));
  }
}
```

`REC_SPAN_BEGIN`: `{ span_id: u16, sym: u16, parent_span_id: u16, task_id: u8 }`
`REC_SPAN_END`: `{ span_id: u16 }`

**REC-19** — El desktop construye: (a) tabla con conteo, total, media, **p50/p95/p99, max**; (b) timeline por tarea con los spans anidados dibujados como barras; (c) árbol de costo acumulado tipo flamegraph. Esto es cualitativamente distinto del `min/max/avg` de v1 y es lo que efectivamente sirve para encontrar el outlier que te rompe el timing.

**REC-20** — **Los spans abiertos al momento de una pausa se invalidan.** Al entrar en un checkpoint, toda tarea descarta sus spans abiertos y emite `REC_SPAN_ABORT { span_id, reason: PAUSED }` por cada uno; el host los marca como inválidos y los excluye de los histogramas. No se descuenta el tiempo pausado. Reportar 4 segundos de "build_frame" porque el usuario apretó pausa es peor que no reportar nada, y descontar exigiría que cada tarea llevara su propio acumulado de tiempo pausado, lo que se complica con el alcance `Barrier` (FW-20). Se revisa si aparece un caso de uso real que lo justifique. Resuelto en PA-05.

**REC-21** — `span_id` es un **contador global de instancias**, u16 atómico con wrap en 65535, asignado en `BEGIN`. No es por tarea: `REC_SPAN_END` y `REC_SPAN_ABORT` no llevan `task_id`, y con contadores por tarea dos tareas podrían tener el mismo id abierto a la vez, dejando el `END` sin dueño decidible. El host tolera `END` sin `BEGIN` (por drops) descartando el span. Un `BEGIN` cuyo `span_id` ya figura abierto descarta la instancia anterior como stale (caso wrap con un span largo abierto).

### 7.7 Counters (REC-22)

```cpp
mole.count("irq_rx");        // barato: incrementa un uint32 local
```

**REC-23** — Los counters **se agregan en el MCU** y se emiten como delta cada 250 ms. Un contador incrementado 100.000 veces por segundo cuesta 4 registros por segundo en el wire. Es la forma correcta de instrumentar una ISR.

**REC-24** — `mole.count()` DEBE ser seguro desde ISR (incremento atómico, sin cola).

`REC_COUNTER`: `{ n: u8, entries: [{sym: u16, delta: u32}] }` — batcheado en un solo record.

### 7.8 Máquinas de estado y status (REC-25)

```cpp
mole.state("link_fsm", "JOINING");
mole.state("link_fsm", "JOINED");
```

El desktop dibuja una **lane temporal** por máquina, con la duración de cada estado y las transiciones. Para debuggear un stack LoRa o un protocolo de handshake es exactamente la vista que uno quiere y ninguna herramienta actual da.

`REC_STATUS` mantiene el LED de v1 (OFF/verde/amarillo/rojo) como indicador de salud simple.

### 7.9 Blobs (REC-26)

Para payloads > 255 bytes (una imagen de cámara, un dump de flash, un frame de radar).

`REC_BLOB_BEGIN`: `{ blob_id: u16, sym: u16, total_len: u32, schema_sym: u16 }`
`REC_BLOB_CHUNK`: `{ blob_id: u16, offset: u32, data[] }`

**REC-27** — Los chunks se intercalan con el resto del tráfico; nunca monopolizan el enlace. La `moleTask` DEBE limitar los blobs a un porcentaje configurable del ancho de banda (default 50%).

### 7.10 Checks (REC-28)

```cpp
MOLE_CHECK(len <= sizeof(buf));
MOLE_CHECK_MSG(crc_ok, "crc esperado={:04X} recibido={:04X}", exp, got);
```

Los checks usan el **mismo pipeline de formateo diferido** que los logs (§7.2). Un camino printf-style acá volvería a linkear `vsnprintf` y anularía PERF-15, y los argumentos tipados (REC-39) valen más que nunca en un fallo esporádico: filtrar por el valor de `got` es lo que muestra el patrón.

- `MOLE_CHECK(cond)` usa `#cond` como format string, sin argumentos.
- `MOLE_CHECK_MSG(cond, fmt, ...)` concatena `#cond` y `fmt` como literales adyacentes; el resultado sigue siendo un literal, así que la verificación en compilación de REC-33 aplica intacta.

Un check fallido genera `REC_CHECK_FAIL { fmt_id: u16, task_id: u8, core: u8, args[] }` con **flag de urgencia** (fuerza flush inmediato del frame, PR-05c). `file_sym` y `line` viajan en el `REC_FMT_DEF`, que en un check se emite en el primer fallo, dentro del mismo frame urgente: el host siempre tiene la definición antes del record que la referencia. Opcionalmente dispara pausa automática si el desktop lo pidió (`break on check fail`).

### 7.11 Comandos (REC-29)

```cpp
mole.command("Reset radio", []() { radio.reset(); });
mole.command("Set channel", [](int ch) { radio.setChannel(ch); }, 0, 15);
mole.command("Send text", [](const char* s) { radio.send(s); });
```

`REC_CMD_DEF`: `{ cmd_id: u16, sym: u16, arg_type: u8, min[], max[] }`

**REC-30** — La UI se genera desde el firmware, sin configuración en el desktop. Argumento tipado ⇒ el desktop renderiza el control adecuado (botón, slider, campo numérico, dropdown, texto).

**REC-31** — El callback del comando se ejecuta en el punto seguro (§8.6), en el contexto de la `moleTask` por defecto, o en la tarea que llame a `mole.dispatch()` si se registró con afinidad de tarea.

---

## 8. Librería de firmware

### 8.1 Estructura interna (FW-01)

```
mole/
  include/mole.h            API pública
  src/mole_core.cpp         intern, ring buffers, políticas
  src/mole_task.cpp         moleTask: batching, framing, TX, RX, dispatch
  src/mole_codec.cpp        COBS + CRC32 (compartido con vectores de test)
  src/mole_transport_uart.cpp
  src/mole_transport_cdc.cpp
  src/mole_transport_tcp.cpp
  src/mole_pause.cpp        checkpoints, EventGroup de control
```

**FW-02** — Distribución: componente de ESP-IDF + librería de PlatformIO + `library.json`. Un solo árbol de fuentes.

### 8.2 Colas y concurrencia (FW-03)

**FW-04** — Un ring buffer **SPSC lock-free por tarea productora**, no una cola compartida. Registro implícito en el primer uso desde una tarea nueva (`pvTaskGetThreadLocalStoragePointer`). Elimina la contención de mutex del hot path, que es lo que compra PERF-01/02.

**FW-05** — Las ISRs escriben en un ring buffer dedicado, sin bloqueo. Solo se permiten desde ISR: `count()`, `event()`, y un `log()` acotado sin formateo (`mole.isrLog(sym)`).

**FW-06** — La `moleTask` hace round-robin sobre los rings de productores, arma el frame y lo emite. Prioridad configurable (default: `tskIDLE_PRIORITY + 2`), stack 4096, **pineada al core 0** por defecto (el core de "sistema" en configuraciones Arduino, donde `loopTask` corre en el 1).

**FW-07** — El orden temporal global se reconstruye en el host ordenando por timestamp absoluto (`t_base_us + dt_us`), no por orden de llegada. Con rings por tarea, el orden de arribo no es cronológico.

**FW-08** — Ningún productor bloquea con `Block` por más de `MOLE_BLOCK_TIMEOUT_MS` (default 10 ms), tras lo cual descarta y contabiliza. Un debugger no puede colgar el firmware que está debuggeando.

### 8.3 Memoria (FW-09)

| Buffer | Default | Configurable |
|---|---|---|
| Ring por tarea productora | 4 KB | `MOLE_RING_SIZE` |
| Ring de ISR | 1 KB | `MOLE_ISR_RING_SIZE` |
| Buffer de armado de frame | 4 KB | `MOLE_FRAME_MAX` |
| Buffer de COBS | 4 KB + 32 | derivado |
| Tabla de símbolos | 512 × 8 B = 4 KB | `MOLE_MAX_SYMBOLS` |

**FW-10** — Con PSRAM disponible, los rings PUEDEN alocarse ahí. Sin PSRAM, la config por defecto con 3 tareas productoras ronda 20 KB (PERF-12).

### 8.4 Compilación condicional (FW-11)

**FW-12** — `MOLE_ENABLED=0` DEBE compilar todas las macros y llamadas a nada (`(void)0`), sin dejar ni strings ni símbolos en el binario. Requisito para poder dejar la instrumentación puesta en el código y compilar release sin costo.

**FW-13** — Niveles de compilación por canal: `MOLE_LEVEL_MIN`, `MOLE_SPANS_ENABLED`, `MOLE_DUMPS_ENABLED`, `MOLE_LOG_SOURCE`.

### 8.5 API pública (esbozo) (FW-14)

```cpp
namespace mole {

struct Config {
  Transport transport   = Transport::Auto;   // Auto elige CDC nativo si existe
  uint32_t  baud        = 921600;            // solo UART
  uint16_t  frame_max   = 4096;
  uint8_t   flush_ms    = 5;
  uint8_t   task_prio   = tskIDLE_PRIORITY + 2;
  int8_t    task_core   = 0;
  bool      block_until_connected = false;
};

void begin(const Config& cfg = {});
void end();
void flush();

// logs — formateo diferido, sintaxis de llaves, verificado en compilación
#define MOLE_TRACE/MOLE_DEBUG/MOLE_INFO/MOLE_WARN/MOLE_ERROR/MOLE_FATAL(fmt, ...)
#define MOLE_INFO_T(tag, fmt, ...)                  // variante con tag
void logs(Level lvl, const char* msg);              // fallback: formatea en el MCU
// watch
template<class T> void watch(const char* label, T value);
template<class T> void watchEvery(const char* label, T value, uint32_t ms);
// bind
template<class T> void bind(const char* label, T* ptr, T min, T max, T step = {},
                            void(*on_change)() = nullptr);
// descriptores de tipo (§7.2.4) — no intrusivos, al lado del struct
#define MOLE_DESCRIBE(T, ...)            // campos, orden natural
#define MOLE_DESCRIBE_BE(T, ...)         // campos big-endian (registros de sensor)
#define MOLE_DESCRIBE_ENUM(E, ...)       // valores por nombre
#define MOLE_FIELD_ARRAY(T, campo, n)    // campo arreglo
#define MOLE_TYPE(T)                     // TypeId del descriptor de T

// dump
void dump(const char* label, const void* data, size_t len, TypeId ty = NO_TYPE);
void schema(const char* name, const char* definition);   // DSL, layouts sin struct detrás
#define MOLE_DUMP(obj)                   // vista de bytes con padding a la vista
#define MOLE_BYTES(obj)                  // hexdump inline sin descriptor
// spans
#define MOLE_SPAN(name)  mole::ScopedSpan _mole_span_##__LINE__(name)
void spanBegin(const char* name); void spanEnd(const char* name);
// counters / eventos / fsm
void count(const char* name, uint32_t n = 1);
void event(const char* name, uint32_t arg = 0);
void state(const char* machine, const char* state);
void status(const char* name, Level l);
// comandos
void command(const char* label, void(*fn)());
template<class A> void command(const char* label, void(*fn)(A), A min, A max);
// pausa
void checkpoint();
void pause(const char* reason = nullptr);
bool isPaused();
// checks
#define MOLE_CHECK(cond) ...
// housekeeping
void dispatch();            // ejecuta callbacks pendientes en la tarea llamante
Stats stats();

}
```

**FW-15** — No hay instancia global `mole` de clase con estado mutable compartido (patrón de v1 que causaba la corrupción de buffers). El estado vive en un singleton interno del `.cpp`; la API es de funciones libres en el namespace.

### 8.6 Pausa cooperativa y puntos seguros (FW-16)

Recoge el diseño discutido: **checkpoint cooperativo por tarea**, no `vTaskSuspend()`.

**FW-17** — La `moleTask` mantiene un `EventGroupHandle_t` de control con bits: `PAUSE_REQ`, `RESUME`, `STEP`, y un bit por tarea participante para la barrera.

**FW-18** — `mole::checkpoint()` es la primitiva. Costo en camino frío: leer los bits del event group (~100 ns). Cuando hay pausa pedida:

1. Se des-suscribe del TWDT (`esp_task_wdt_delete(NULL)`) — más honesto que llamar a `esp_task_wdt_reset()` a ciegas como v1, que sobre una tarea no suscripta simplemente devuelve error y no hace nada.
2. Emite `REC_PAUSED { task_id, file_sym, line, reason }` con flag de urgencia.
3. Espera `RESUME`/`STEP` con **timeout obligatorio** (`MOLE_PAUSE_TIMEOUT_MS`, default 300.000 = 5 min). Al vencer, reanuda sola y emite `REC_RESUMED` con motivo `TIMEOUT`. Sin esto, cerrar la app con el MCU pausado deja una tarea muerta para siempre.
4. Se re-suscribe al TWDT y emite `REC_RESUMED`.

**FW-19** — `mole::pause()` es un checkpoint incondicional (breakpoint en código).

**FW-20** — Alcances de pausa (`scope` en `CTL_PAUSE`):

| Scope | Comportamiento |
|---|---|
| `Task` | Solo la tarea indicada se detiene en su próximo checkpoint |
| `All` | Todas las tareas participantes, cada una en su checkpoint |
| `Barrier` | Como `All`, pero el desktop muestra "3/5 detenidas" hasta converger; con timeout de convergencia |

**FW-21** — `vTaskSuspend()` desde afuera **NO se implementa**. Suspender una tarea que tiene tomado un mutex produce inversión de prioridad o deadlock; en el peor caso, la tarea suspendida tiene el mutex de Mole y el debugger se cuelga a sí mismo.

**FW-22** — El desktop DEBE mostrar explícitamente qué **no** se pausa: ISRs, stack de Wi-Fi/BLE, timers de hardware, DMA. Es la diferencia real con un breakpoint de JTAG y el usuario tiene que tenerla clara. Un cartel en el panel de pausa, no una nota al pie de la documentación.

**FW-23** — Punto seguro para efectos de downlink: los callbacks de comandos y las escrituras de bind se aplican (a) en la `moleTask` por defecto, o (b) en la tarea que llame a `mole::dispatch()`. En una tarea pausada en un checkpoint, `dispatch()` se llama automáticamente antes de reanudar, de modo que "editá el threshold y seguí" funciona como uno espera.

---

## 9. Desktop: núcleo Rust

### 9.1 Pipeline (HOST-01)

```
hilo transporte  → bytes crudos → canal (crossbeam, bounded)
hilo decoder     → COBS + CRC + parse → Vec<Record> por lote
hilo store       → escribe en el store columnar
hilo coalescer   → cada 33 ms arma un snapshot delta → IPC → UI
```

**HOST-02** — El decoder NUNCA emite un evento de IPC por record. En v1 cada paquete hacía `app_handle.emit()` con serialización JSON y cruce del bridge al webview. Ese es el cuello de botella que hace imposible PERF-06, y no se arregla optimizando: hay que eliminar el patrón.

**HOST-03** — El decoder DEBE procesar lotes. Objetivo: ≥ 500k records/s en un solo core (holgura de 10× sobre PERF-06).

**HOST-04** — Tolerancia a basura en el stream: al fallar el CRC o el COBS, se descarta hasta el próximo `0x00` y se contabiliza. Sin el hack de v1 de reintentar el decode salteando bytes uno por uno (que era O(n²) y existía porque el propio ejemplo mezclaba `Serial.println()` con el stream binario).

**HOST-05** — Salidas de texto plano no-Mole (el log del bootloader ROM del ESP32, un `printf` suelto) DEBEN detectarse y mostrarse en un canal aparte "raw", nunca descartarse en silencio. En la práctica el 100% de los arranques de ESP32 empieza con basura a otro baudrate; hay que mostrarla, es información.

### 9.2 Store (HOST-06)

**HOST-07** — Store columnar en Rust, append-only, por canal:

| Canal | Estructura |
|---|---|
| Logs | Arena paginada de 1 MB + índice `(t, level, tag, task)` para filtrado |
| Watches | Ring por símbolo, `Vec<(u64, Value)>`, retención configurable |
| Spans | Arena + índice por símbolo para histogramas |
| Dumps | Arena, con dedup del contenido idéntico consecutivo |
| Counters | Serie por símbolo |
| Estados | Lista de transiciones por máquina |

**HOST-08** — Retención configurable con dos políticas: por cantidad (default 10M records) y por memoria (default 1 GB). Al superar, se hace spill a disco de las páginas viejas (mmap) o se descartan según preferencia del usuario.

**HOST-09** — El filtrado de logs se resuelve sobre columnas (nivel, tag, tarea como enteros) antes de tocar el texto. Esto es lo que compra PERF-11.

### 9.3 Contrato de IPC (HOST-10)

**HOST-11** — Un solo evento periódico a 30 Hz: `mole:tick`, con un payload compacto:

```ts
type Tick = {
  seq: number;
  link: { recPerSec, bytesPerSec, drops, gaps, connected, paused };
  logs: { total: number; newFrom: number; newTo: number };  // solo índices
  watches: Array<{ id, value, min, max, avg, n }>;          // solo los que cambiaron
  spans:   Array<{ id, n, p50, p95, p99, max }>;
  counters: Array<{ id, value, rate }>;
  states:  Array<{ id, current, since }>;
  status:  Array<{ id, level }>;
  newSyms: Array<{ id, kind, parent, name }>;
};
```

**HOST-12** — El detalle se pide bajo demanda: `invoke("log_slice", { from, to, filter })` devuelve un **buffer binario** (`tauri::ipc::Response`), no JSON. El virtual scroller pide solo la ventana visible ± margen.

**HOST-13** — La UI **nunca** acumula el historial completo en memoria de JS. El array de logs de v1 (limitado a 2000 con `shift()` en cada mensaje, que es O(n) por mensaje) desaparece.

**HOST-14** — Todas las ventanas (incluidas las flotantes de detalle) consumen el mismo contrato. En v1 la ventana desprendida generaba **datos random con un mock**, porque era un webview separado sin acceso al estado de `HomeView`. Con el estado en Rust el problema no existe.

---

## 10. Desktop: UI

### 10.1 Principios

**UI-01** — Es una herramienta de desarrollo, no una landing. Densidad alta, tema oscuro por defecto, cero animación en los paneles calientes, todo alcanzable por teclado.

**UI-02** — **La densidad es una decisión de producto, no de estilo.** En v1 las filas de log medían ~53 px: en una pantalla de 1080p entraban 20 líneas. Con 20 px entran 55. Cuando estás buscando la línea donde se rompió algo, esa diferencia es la herramienta.

**UI-03** — Nada en la UI hace trabajo proporcional al volumen de datos. Todo panel opera sobre la ventana visible, servida por el store de Rust (HOST-12). El límite superior de trabajo por cuadro es constante, independiente de si la sesión tiene mil records o diez millones.

### 10.2 Layout y paneles

**UI-04** — **Workspace como árbol de splitters**, con paneles como hojas. Es lo único de la UI de v1 que se rescata tal cual: arrastrar divisores para armar el espacio de trabajo resultó cómodo y se mantiene.

**UI-05** — Cada hoja puede contener **varios paneles en pestañas** (modelo VS Code). Arrastrar una pestaña la mueve entre hojas o crea una división nueva.

**UI-06** — El árbol de layout se persiste y se guarda como **preset con nombre**. Presets de fábrica:

| Preset | Paneles |
|---|---|
| Sensor | Log + Watch + Dump |
| Timing | Spans + Timeline + Log |
| Protocolo | Dump + Raw + Log |
| Control | Binds + Comandos + Status + Log |

Cambiar de preset es un atajo de teclado. Debuggear un sensor y perfilar una tarea no quieren la misma pantalla.

**UI-07** — Cualquier panel se desprende a una ventana propia. A diferencia de v1 —donde la ventana desprendida generaba **datos aleatorios con un mock** porque era un webview aislado— todas las ventanas consumen el mismo contrato de IPC (HOST-14).

**UI-08** — El árbol de splitters se implementa **a mano**, no con un componente de librería. Son unas 300 líneas y necesitamos control fino sobre persistencia, pestañas, arrastre y desprendido. Es además el componente que menos conviene atar a una dependencia externa, porque define el esqueleto de todo.

### 10.3 Estrategia de renderizado por panel

**UI-09** — La decisión de rendering es **por panel**, no global:

| Panel | Técnica | Motivo |
|---|---|---|
| Log | DOM virtualizado | Selección de texto, copiar y accesibilidad salen gratis |
| Dump | DOM virtualizado | Idem, y hay que poder seleccionar bytes |
| Watch / Binds / Comandos / Status | DOM | Decenas de filas, no miles |
| Spans (tabla) | DOM virtualizado | Idem log |
| Timeline global | Canvas | Miles de elementos por cuadro |
| Spans (flamegraph) | Canvas | Idem |
| Lanes de estados | Canvas | Idem |
| Sparklines | Canvas | Uno por fila, decenas por cuadro |

**UI-10** — El log **no** va a canvas. Un log renderizado en canvas obliga a reimplementar selección, copiado, búsqueda del sistema y accesibilidad. Un log virtualizado en DOM con filas de altura fija y posicionamiento absoluto llega de sobra a PERF-09, porque solo existen ~60 filas materializadas.

**UI-11** — Canvas para todo lo que supere ~500 elementos por cuadro. Sin transiciones CSS ni animaciones en paneles calientes.

### 10.4 Panel de Log

**UI-12** — Columnas conmutables, redimensionables y reordenables: tiempo, nivel, tarea, core, tag, origen (`archivo:línea`), mensaje. Por defecto: tiempo, nivel, tag, mensaje.

**UI-13** — **Modos de visualización del tiempo**, conmutables en caliente:

| Modo | Uso |
|---|---|
| Absoluto (`hh:mm:ss.µµµµµµ`) | Correlacionar con eventos externos |
| Relativo al inicio de sesión | Lectura general |
| Delta con la fila anterior | **Encontrar dónde se fue el tiempo** |
| Delta con una fila marcada | Medir un tramo arbitrario |

En la captura de v1 las 30 filas visibles dicen todas `21:34:29`: el timestamp tenía resolución de segundo y se tomaba en el host. Con microsegundos del MCU (PR-10), el modo delta convierte al log en un instrumento de medición.

**UI-14** — **Barra de filtros siempre visible**, no un popup. Niveles como botones segmentados con conteo por nivel, tags como multiselección, tarea, core y texto con conmutador de regex. Se aplica al tipear, sin botón de confirmar. En v1 el filtro estaba detrás de un ícono de embudo y un `OverlayPanel`: un filtro que hay que abrir es un filtro que no se usa.

**UI-15** — **Filtro por campo de struct.** Con los argumentos guardados tipados (REC-49), `s.valid == false` o `s.ax > 2.0` son expresiones de filtro válidas, no búsqueda de texto.

**UI-16** — **Resaltado separado del filtrado.** Reglas de color por patrón que pintan la línea sin ocultar el resto. Filtrar te da las líneas; resaltar te da las líneas **y su contexto**, que suele ser lo que hace falta.

**UI-17** — **Búsqueda distinta de filtro**: resalta coincidencias y navega con n/N sin alterar el conjunto visible.

**UI-18** — Marcadores: marcar una fila, saltar entre marcadores, y usar una marca como origen del delta de tiempo (UI-13).

**UI-19** — Follow/freeze con **congelamiento automático al hacer scroll hacia arriba** y una píldora "N líneas nuevas" para volver al vivo. Sin esto, leer algo mientras el stream corre es imposible.

**UI-20** — **Filas especiales dentro del stream**, no en un contador aparte:

| Fila | Cuándo |
|---|---|
| Marca de descarte | Hubo drops (PR-14): "▲ 1.204 records perdidos" |
| Corte de sesión | Cambió el `app_elf_sha256` o hubo reset (DX-02) |
| Marca de pausa/reanudación | El MCU entró o salió de un checkpoint |

**UI-21** — **Expansión de fila** para logs con structs: la fila se despliega a una tabla de campos sin abrir otra ventana (REC-49). El scroller virtual maneja la altura variable con un mapa de offsets solo para las filas expandidas.

**UI-22** — Copiado: líneas seleccionadas como texto renderizado, o como JSON con los argumentos tipados. La segunda opción es la que sirve para pegar en un issue.

### 10.5 Scroller virtual

**UI-23** — Altura de fila **fija** por nivel de densidad (compacto 18 px / normal 22 px / cómodo 28 px). La altura fija es lo que hace que el cálculo de índice sea O(1).

**UI-24** — Lectura por ventana desde Rust (`log_slice`, HOST-12) con margen de prefetch. Nunca se materializa la sesión completa en JS.

**UI-25** — El scrollbar es **indexado, no pixelado**. Con 10 millones de filas a 22 px el alto real supera el límite de los navegadores; se mapea posición de scroll a índice y se dibuja el pulgar a mano.

**UI-26** — Anclaje: con el stream congelado, la fila superior visible no se mueve aunque lleguen filas nuevas.

### 10.6 Otros paneles

**UI-27** — **Watch.** Tabla con etiqueta, valor, tipo, min/max/media/desvío y sparkline. Formato por fila configurable (hex, decimal, binario, precisión), aplicado **de forma retroactiva** sobre los valores ya capturados (REC-39). Expandible a panel con gráfico.

**UI-28** — **Agrupación jerárquica por nombre.** Los símbolos con punto (`imu.ax`, `imu.ay`, `imu.gz`) se agrupan en un árbol colapsable. Con cincuenta variables en pantalla es la diferencia entre una tabla y una lista inservible.

**UI-29** — **Binds.** Controles generados según tipo. Cada control muestra estado **sucio** cuando el valor local difiere del confirmado por el dispositivo, con opción de revertir. Sin eso no se sabe si el MCU recibió el cambio.

**UI-30** — **Dump.** Grilla hexadecimal con offsets, ASCII lateral, resaltado de diff, overlay de schema al costado y fijado de referencia. Selección de bytes con lectura del valor interpretado según el tipo elegido.

**UI-31** — **Spans.** Tabla con conteo, total, media, p50/p95/p99 y máximo, más flamegraph en canvas. Los spans invalidados por pausa (REC-20) se muestran tachados y excluidos de los percentiles.

**UI-32** — **Timeline global.** Franja fija arriba, en canvas: densidad de log por nivel como mapa de calor, barras de spans por tarea, lanes de estados, marcadores y descartes. Hacer scrub mueve **todos** los paneles al mismo instante. Es lo que convierte varias tablas en un debugger.

**UI-33** — **Raw.** El texto no-Mole del stream (HOST-05), incluido el arranque del bootloader ROM, en su propio panel.

### 10.7 Barra superior y estado

**UI-34** — La barra de conexión muestra baudrate **solo si el transporte es UART**. En USB-CDC nativo el baudrate no existe y ofrecerlo confunde.

**UI-35** — Salud del enlace siempre visible, con números y no con un semáforo: rec/s, KB/s, drops acumulados, huecos de secuencia, RTT. En v1 la barra de estado decía "Ready" de forma permanente.

**UI-36** — Controles de ejecución agrupados y con estado real: pausar, reanudar, paso, reset. En v1 los botones de play/pausa **no tenían handler**.

**UI-37** — Identidad del dispositivo visible: chip, revisión, nombre de la app, hora de build. Sirve para darse cuenta de que se está mirando un firmware viejo.

### 10.8 Interacción

**UI-38** — **Paleta de comandos** (`Cmd/Ctrl+K`) con acceso a todo: cambiar preset, conmutar panel, filtrar, ejecutar un comando del firmware, saltar a un marcador. En una herramienta de desarrollo se da por sentada.

**UI-39** — Atajos para lo frecuente: pausa/reanudar/paso, limpiar, follow, foco en filtro, buscar, marcar, siguiente marcador, cambiar modo de tiempo.

**UI-40** — Todo filtro, resaltado, disposición y selección es **estado serializable**, guardable junto con la sesión y compartible.

**UI-41** — Los niveles se distinguen por **forma e ícono además de color**, nunca solo por color.

### 10.9 Stack

**UI-42** — La elección de framework es **secundaria frente a la estrategia de renderizado**. Como la UI recibe un tick coalescido a 30 Hz (HOST-11) y no un evento por record, la carga por cuadro es de unas cientos de actualizaciones de DOM. Cualquier framework moderno la sostiene **si las actualizaciones son quirúrgicas**. Lo que no la sostiene es un componente de tabla de propósito general con filas caras.

**UI-43** — **Los paneles calientes se escriben a mano, sin librería de UI.** Log, dump, spans, timeline, sparklines y el árbol de splitters. Es el ~30% de la superficie y el 95% del riesgo de performance.

**UI-44** — La librería de UI solo se usa en la parte fría: barra superior, diálogos, preferencias, controles de formulario de los binds. Preferentemente **headless** (comportamiento y accesibilidad sin estilos impuestos), para que la densidad de UI-02 sea alcanzable sin pelear contra el CSS de la librería.

**UI-45** — Requisitos que el framework DEBE cumplir:

| Requisito | Motivo |
|---|---|
| Reactividad de grano fino | Actualizar 50 filas de watch no debe recorrer el árbol entero |
| Sin VDOM, o con escape explícito | El diff por cuadro es trabajo puro contra PERF-09 |
| Control directo del DOM cuando hace falta | El scroller y la grilla hexadecimal se manejan a mano |
| Runtime chico | Arranque del ejecutable |
| Buen interop con canvas | Cuatro paneles son canvas |

Candidatos evaluados: Svelte 5 (runes), SolidJS, Vue 3 con `shallowRef`. Los tres cumplen. React se descarta: su modelo de re-render es el peor encaje para actualizaciones de alta frecuencia. **Ver PA-12.**

---

## 11. Sesiones: grabación y replay

**SES-01** — Formato `.molecap`: header con metadata (`REC_SESSION`, transporte, fecha, versión) + **stream crudo pre-COBS** tal como llegó, con timestamps de arribo del host.

**SES-02** — Grabación siempre activa a un buffer circular en disco (default 500 MB). "Guardar los últimos 5 minutos" tiene que ser un botón, no algo que había que haber activado antes del bug.

**SES-03** — Replay a 1×, a velocidad máxima, o paso a paso. El pipeline del host no distingue entre transporte real y replay (TR-06).

**SES-04** — El replay habilita desarrollar la app desktop sin hardware, y **compartir un bug**: mandás el `.molecap` y el otro ve exactamente lo mismo.

**SES-05** — Exportación: logs a texto/JSONL, watches y spans a CSV/Parquet, dumps a binario.

---

## 12. DX y flujo de trabajo

**DX-01** — **Flash guard.** El desktop detecta que se está por flashear y libera el puerto:
- Detección primaria: aparición de un proceso `esptool`/`esptool.py`/`pio` (polling de la tabla de procesos cada 250 ms).
- Detección secundaria: fallo de escritura por puerto ocupado ⇒ soltar y entrar en modo reconexión.
- En USB-CDC nativo el device desaparece del bus al resetear; se maneja con TR-09.

**DX-02** — Reconexión automática con retención del catálogo. Al volver, se manda `CTL_HELLO` con el `epoch`/`catalog_hash` conocidos; si el firmware es el mismo, la sesión continúa sin perder el historial. Si cambió el `app_elf_sha256`, se marca una división de sesión visible en la timeline.

**DX-03** — Autodetección de puerto: si hay un solo dispositivo con VID/PID conocido de Espressif o de puente USB-serial, se preselecciona. El default de baudrate de la UI DEBE coincidir con el de la librería (en v1 la UI arrancaba en 19200 y los ejemplos en 115200, garantizando que la primera ejecución no funcionara).

**DX-04** — Modo demo: un firmware simulado embebido en el desktop genera tráfico sintético representativo. Sirve para evaluar la herramienta sin hardware y para tests de UI.

**DX-05** — Snippets copiables: cada símbolo en la UI ofrece "copiar la línea de C++ que lo produce".

---

## 13. Seguridad y robustez

**SEC-01** — El transporte TCP (TR-04) DEBE tener un token compartido opcional (`Config::auth_token`). Un puerto abierto que permite ejecutar comandos y reiniciar el dispositivo no puede estar sin protección en una red de campo.

**SEC-02** — `CTL_RESET` requiere el magic `0x4D4F4C45`. En v1 un byte suelto `0x84` en la línea reiniciaba el MCU.

**SEC-03** — El firmware DEBE ignorar todo downlink hasta haber completado un handshake válido.

**SEC-04** — El desktop DEBE tratar todo lo que llega del MCU como no confiable: strings sin terminador, longitudes mentirosas, `sym_id` inexistentes. Ningún panic. Fuzzing obligatorio del decoder (§15).

---

## 14. Comparación con el estado del arte

| | Serial Studio | Teleplot | Better SP | **Mole v2** |
|---|---|---|---|---|
| Plot de series | ★★★ | ★★★ | ★★★ | ★ (deliberado) |
| Logs estructurados | ★ | — | — | ★★★ |
| Hexdump + diff + schema | — | — | — | ★★★ |
| Profiling con percentiles | — | — | — | ★★★ |
| Spans anidados / timeline | — | — | — | ★★★ |
| Bind de variables en vivo | — | — | — | ★★★ |
| Pausa cooperativa | — | — | — | ★★★ |
| UI declarada desde firmware | ★ | — | — | ★★★ |
| Throughput | media | baja (texto) | media | alta (binario batcheado) |
| Grabación / replay | ★ | — | — | ★★★ |

**El nicho es "debugger de firmware sin JTAG", no "osciloscopio serial".** Ninguna de las tres alternativas juega ahí.

---

## 15. Testing

**TEST-01** — **Vectores compartidos.** Un archivo `codec_vectors.json` en el repo con pares (frame, bytes COBS+CRC) generado una vez, consumido por los tests de Rust y de C++. Es la única forma de que dos implementaciones del mismo protocolo no vuelvan a divergir en silencio (en v1 el downlink divergió y nadie se enteró porque nunca se ejecutó).

**TEST-09** — Tests de descriptores: `offsetof`/`sizeof` esperados contra el layout real del compilador para structs con padding, anidados, arreglos y enums, en las variantes de alineación de Xtensa y RISC-V. Un descriptor que miente sobre el layout hace que el modo bytes muestre basura convincente, que es peor que un error.

**TEST-08** — El formateador del host tiene su propio set de vectores: `(fmt, arg_types, arg_bytes) → string esperado`, cubriendo precisión, bases, anchos, negativos, `NaN`/`Inf`, cadenas internadas y de runtime, y `argc` en desacuerdo con el `REC_FMT_DEF` (que DEBE renderizar un placeholder de error, no entrar en pánico).

**TEST-02** — Tests unitarios de C++ corriendo en host (no en el MCU) con el core compilado como librería nativa. COBS, CRC32, intern, políticas de ring.

**TEST-03** — Fuzzing del decoder de Rust con `cargo-fuzz`. Criterio: cero panics con 24 h de fuzzing.

**TEST-04** — Test de integración con hardware real: firmware de banco que emite a tasa creciente y verifica en el host que la cuenta recibida + drops reportados = cuenta emitida. **Ninguna pérdida no contabilizada.**

**TEST-05** — Benchmark reproducible (`molectl bench`) que reporta la tabla de §4 y falla si se degrada más de 10% respecto de la línea base.

**TEST-06** — Tests de UI con sesiones `.molecap` fijas como fixtures.

**TEST-07** — Medición de PERF-01/02/03 en hardware con GPIO toggling y analizador lógico, documentada, no estimada.

---

## 16. Fases

| Fase | Contenido | Criterio de salida |
|---|---|---|
| **F0** | Congelar §6 y §7. Codec en Rust y C++ + vectores compartidos. | TEST-01, TEST-02 en verde |
| **F1** | Firmware core: `moleTask`, rings, símbolos, log/watch/counter. Host headless `molectl`. | PERF-04/05/06 medidos |
| **F2** | Desktop: transporte, store, coalescer, vistas de Log y Watch. | PERF-08/09/11 medidos |
| **F3** | Comandos + Bind. Ciclo interactivo completo. | Prototipar un sensor real sin recompilar |
| **F4** | Dump + schema + diff. Checks. | Debuggear un I2C nuevo end-to-end |
| **F5** | Spans + timeline + percentiles. Estados. | Encontrar un outlier de timing real |
| **F6** | Pausa cooperativa, checkpoints, step. | §8.6 completo |
| **F7** | Sesiones, replay, flash guard, autodetección. | DX-01..05 |
| **F8** | Transporte TCP, mDNS, auth. | TR-04 |

**F1 antes que cualquier UI**: si el core no llega a los números de §4, todo lo de arriba se rediseña. Es el mismo criterio del *green backend test gate* del ERP.

---

## 17. Puntos abiertos

### 17.1 Pendientes de decisión

**PA-03 — USB vendor bulk (TR-05).** ¿Se implementa desde el principio o se espera a medir CDC? Implica driver con `nusb` y, en Windows, instalación de WinUSB (fricción real para terceros).

**PA-12 — Framework de UI.** Los tres candidatos cumplen UI-45 y la diferencia real entre ellos es menor que la diferencia entre una tabla hecha a mano y una de librería (UI-42). El criterio de desempate es de proyecto, no técnico:

| Opción | A favor | En contra |
|---|---|---|
| **Svelte 5 (runes)** | Runtime mínimo, reactividad de grano fino, DX buena, sin VDOM | Corte con el stack del ERP; librerías headless jóvenes (Melt/Bits) |
| **SolidJS** | El más rápido en actualizaciones quirúrgicas, señales que calzan exacto con el tick por deltas | Ecosistema chico, menos gente, JSX en un proyecto sin React |
| **Vue 3 + `shallowRef`** | Fluidez ya adquirida, idiomas compartidos con el ERP | Hay que desactivar la reactividad profunda a mano; en v1 ya aparecían `triggerRef` manuales, señal de estar peleando con el framework |

Recomendación: **Svelte 5** si se acepta el costo de contexto, porque es un proyecto nuevo sin restricciones de compatibilidad y los paneles calientes se escriben a mano de todas formas. **Vue 3** si se prioriza velocidad de arranque y compartir idiomas con el resto del trabajo. Decidir en F2, no antes: F0 y F1 no tocan UI.

**PA-06 — Retención por defecto y spill a disco.** ¿Spill transparente (más complejo, nunca perdés nada) o descarte con aviso (simple, honesto)? Afecta PERF-10.

### 17.2 Resueltos

**PA-01 — ~~Deferred formatting de logs.~~ RESUELTO (draft.4): a favor, con sintaxis de llaves.** El MCU emite `fmt_id` + argumentos binarios (`REC_LOG_FMT`) y el host formatea. Se elige `{}` sobre `printf` porque el tipo lo aporta C++ y no el string, lo que elimina la clase de bug del `%d` con float y hace que el formateador del host sea casi un mapeo directo al de Rust. Entra en **v2.0, no en v2.1**: agregarlo después no rompe el formato, pero obligaría a migrar todos los call sites ya escritos en el otro estilo. Costos aceptados: format string obligatoriamente literal, un formateador de runtime en Rust (~200 líneas, porque `format!` exige literal en compilación), instanciación de templates a vigilar en flash, y dos records de log conviviendo de forma permanente. Ver §7.2.

**PA-02 — ~~¿Solo ESP32?~~ RESUELTO (draft.2): solo ESP32.** No se construye capa de abstracción de plataforma. La librería usa APIs de ESP-IDF directamente (`esp_timer_get_time`, `esp_rom_crc32_le`, `esp_task_wdt_*`, TinyUSB). Si en el futuro aparece la necesidad de portar, se paga el refactor entonces; abstraer preventivamente cuesta complejidad hoy contra un beneficio hipotético. Ver PLAT-05.

**PA-04 — ~~Generación de schemas desde structs.~~ RESUELTO (draft.5): macro variádica, fuente única.** `MOLE_DESCRIBE` genera un descriptor `constexpr` no intrusivo, hallado por ADL, y ese mismo descriptor alimenta el logueo de structs (§7.2.4) y el overlay de dumps (REC-16). La duplicación entre el struct y el string del schema desaparece. El DSL de texto sobrevive como camino secundario para layouts sin struct detrás (REC-50) y para bitfields (REC-51). Se descarta la opción X-macro por incómoda en el punto de declaración.

**PA-05 — ~~Semántica de spans durante la pausa.~~ RESUELTO (draft.3): invalidar.** Los spans abiertos se descartan y se emite `REC_SPAN_ABORT`. Descontar el tiempo pausado es más útil en teoría pero exige que cada tarea lleve su propio acumulado de pausa, lo que se enreda con el alcance `Barrier`, y el caso de uso todavía es hipotético. Decisión reversible: el host ya recibe el `REC_SPAN_ABORT`, así que cambiar a descuento no rompe el formato. Ver REC-20.

**PA-07 — ~~Uso del ELF.~~ RESUELTO (draft.2): diferido a v3.** No entra en v2. `app_elf_sha256` se mantiene en `REC_SESSION` (CAT-12) porque cuesta 8 bytes una vez por sesión y es el anclaje que v3 va a necesitar. Ver §19.

**PA-08 — ~~Integración con PampaLink.~~ RESUELTO (draft.2): descartado.** Los dos proyectos mantienen codecs separados. El parecido conceptual (catálogo + announce + resync) no justifica acoplar dos protocolos con restricciones opuestas: PampaLink optimiza para paquetes de decenas de bytes sobre LoRa con ciclo de trabajo limitado, Mole para cientos de KB/s sobre USB. Una librería compartida terminaría siendo el mínimo común denominador de ambos. El patrón se reutiliza; el código no.

**PA-09 — ~~Licencia.~~ RESUELTO (draft.2): MIT.** Librería y aplicación desktop, todo MIT, igual que v1. Sin edición paga ni doble licencia. Archivo `LICENSE` en la raíz y encabezado SPDX en los fuentes de la librería.

**PA-10 — ~~Nombre e identificador.~~ RESUELTO (draft.2): `mole-app`.** `productName: "mole-app"`, binario `mole-app`, CLI headless `molectl`, componente de firmware `mole`. Identificador de bundle propuesto: `net.elesis.mole-app` (reverse del dominio). Se corrige en `tauri.conf.json` y `package.json` en F2, junto con el resto del scaffold heredado.

**PA-11 — ~~Modo "solo texto".~~ RESUELTO (draft.2): descartado.** No se soporta un modo degradado tipo Teleplot. El riesgo identificado se confirma: sería el camino de menor fricción, se volvería el default de hecho, y la mitad de las features del catálogo (§1.4) no existen sin protocolo binario — timestamps del MCU, spans anidados, bind, dumps con schema, contabilidad de pérdida. Un modo que no puede sostener la tesis del producto solo la diluye.

---

## 18. Descartado explícitamente

Para que no vuelvan a aparecer en un debate futuro:

- Compatibilidad con AVR, ESP8266 y el protocolo v1.
- Hashes para identificar símbolos (colisiones silenciosas).
- Un frame por dato.
- `vTaskSuspend()` para implementar pausa.
- Estado del programa viviendo en JavaScript.
- Un evento de IPC por mensaje recibido.
- CRC8.
- Timestamping en el host.
- Constructor de UI por drag-and-drop en el desktop: la UI se declara en el firmware, y eso es la tesis.
- Soporte multiplataforma de MCU y capa de abstracción de plataforma (PA-02).
- Modo degradado de texto plano tipo Teleplot (PA-11).
- Codec compartido con PampaLink (PA-08).
- Licencia dual o edición paga: todo MIT (PA-09).
- Format strings estilo `printf` y formateo de logs en el MCU como camino por defecto (PA-01).
- Escribir el layout de un struct dos veces: en la definición de C++ y en un string de schema (PA-04).
- React como framework de UI: su modelo de re-render es el peor encaje para actualizaciones de alta frecuencia (UI-45).
- Renderizar el log en canvas: obliga a reimplementar selección, copiado, búsqueda y accesibilidad (UI-10).
- Componentes de tabla de propósito general en los paneles calientes (UI-43).
- Filtros escondidos detrás de un popup (UI-14).
- Descriptores intrusivos: nada de heredar de una base, declarar miembros extra ni registrar en tiempo de arranque (REC-42).

---

## 19. Diferido a v3

No se implementa en v2, pero el diseño de v2 no debe cerrarles la puerta.

**V3-01 — Explotación del ELF.** Con el `app_elf_sha256` que v2 ya transporta (CAT-12), el desktop localiza el ELF del build y abre una vertical entera: resolver `file:line` sin gastar bytes en el wire, decodificar backtraces de panic y guru meditation, resolver layouts de struct desde DWARF (lo que resolvería PA-04 de arriba), y mostrar nombres de símbolos en dumps de memoria. Requisito que v2 debe respetar: `REC_SESSION` mantiene el campo aunque no se use.

**V3-02 — Watchpoints.** Detectar escritura sobre una dirección usando el debug hardware del Xtensa/RISC-V sin ocupar el puerto JTAG.

**V3-03 — Coordinación multi-dispositivo.** Varios MCU en una sesión con relojes correlacionados, para debuggear una red LoRa o un bus ESP-NOW desde una sola timeline.
