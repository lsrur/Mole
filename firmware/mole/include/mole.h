// SPDX-License-Identifier: MIT
// mole.h — API pública de Mole v2 (spec §8.5). En F1: logs diferidos,
// descriptores, watch, counters, eventos. Sin instancia global con estado
// compartido (FW-15): funciones libres, el estado vive en el .cpp.
#pragma once

#include "mole_config.h"
#include "mole_wire.h"

#include "mole_describe.h"
#include "mole_log.h"

#if MOLE_ENABLED

#include <stddef.h>
#include <stdint.h>

namespace mole {

// Registro de símbolos (CAT-01/02). El costo en régimen del hot path es el
// load de un static local que guarda el id ya resuelto. Declarada en
// mole_log.h; acá se agrega el default de parent.
// CONTRATO: `name` DEBE apuntar a memoria estable (un literal); no se copia.
// Las macros lo garantizan por construcción.
SymId intern(const char* name, SymKind kind, SymId parent = kSymNone);

// Configuración de arranque (§8.5). Auto elige CDC nativo si existe.
enum class Transport : uint8_t { Auto = 0, Uart = 1, UsbCdc = 2 };

struct Config {
    Transport transport = Transport::Auto;
    uint32_t baud = 921600;  // solo UART (TR-01)
    uint8_t task_prio = 0;   // 0 = default (tskIDLE_PRIORITY + 2, FW-06)
    int8_t task_core = 0;    // FW-06: core 0 por defecto
};

// Arranca la moleTask, única dueña del transporte (ARQ-01). Solo target.
void begin(const Config& cfg = {});
void end();
// Cierra y emite el frame abierto (PR-05 d). Asíncrono: marca y sigue.
void flush();

// Fallback: mensaje ya formateado en el MCU (REC-36). Se accede por acá,
// nunca por las macros. Para mensajes armados en runtime.
void logs(Level lvl, const char* msg);

// Nivel mínimo global y por tag (PR-19); los cambia CTL_SET_LEVEL en vivo.
void set_min_level(Level lvl);
void set_tag_level(SymId tag, Level lvl);

namespace detail {
// REC_WATCH {sym, type, value[]} ya empaquetado
bool watch_emit(SymId sym, uint8_t wire, const void* bytes, uint8_t n);
bool watch_emit_str(SymId sym, const char* s);
// rate limit de watchEvery (REC-09): true si este sym puede emitir ahora
bool watch_rate_ok(SymId sym, uint32_t every_ms);
}  // namespace detail

// Watch de variables (REC-06). El valor viaja auto-descripto.
template <class T>
inline void watch(const char* label, T value) {
    const SymId s = intern(label, KIND_WATCH);
    if (s == kSymOverflow) return;
    if constexpr (detail::is_char_ptr_v<T> || detail::is_char_array_v<T>) {
        detail::watch_emit_str(s, value);
    } else {
        static_assert(std::is_arithmetic_v<std::remove_cv_t<T>>,
                      "MOLE: watch() acepta escalares o cadenas (REC-06)");
        const uint8_t w = detail::scalar_wire<T>();
        if constexpr (std::is_same_v<std::remove_cv_t<T>, bool>) {
            const uint8_t b = value ? 1 : 0;
            detail::watch_emit(s, w, &b, 1);
        } else {
            detail::watch_emit(s, w, &value, sizeof value);
        }
    }
}

// Watch con rate limit en el productor (REC-09): para loops de control de
// alta frecuencia. El descarte por rate limit es deliberado: NO cuenta como
// pérdida (FEAT-34 contabiliza lo involuntario).
template <class T>
inline void watchEvery(const char* label, T value, uint32_t ms) {
    const SymId s = intern(label, KIND_WATCH);
    if (s == kSymOverflow || !detail::watch_rate_ok(s, ms)) return;
    if constexpr (std::is_same_v<std::remove_cv_t<T>, bool>) {
        const uint8_t b = value ? 1 : 0;
        detail::watch_emit(s, detail::scalar_wire<T>(), &b, 1);
    } else {
        detail::watch_emit(s, detail::scalar_wire<T>(), &value, sizeof value);
    }
}

// ---- spans (REC-18..21): medición de bloques con RAII, anidables ----

namespace detail {
uint16_t span_begin(SymId sym);
void span_end(uint16_t span_id);
}  // namespace detail

class ScopedSpan {
  public:
    explicit ScopedSpan(SymId sym) : id_(detail::span_begin(sym)) {}
    ~ScopedSpan() { detail::span_end(id_); }
    ScopedSpan(const ScopedSpan&) = delete;
    ScopedSpan& operator=(const ScopedSpan&) = delete;

  private:
    uint16_t id_;
};

// ---- comandos (REC-29..31): la UI se genera desde el firmware ----

// Sin argumento: botón en el desktop.
void command(const char* label, void (*fn)());
// Con argumento numérico y rango: slider/campo numérico.
void command(const char* label, void (*fn)(int32_t), int32_t min, int32_t max);
void command(const char* label, void (*fn)(float), float min, float max);

// Contador agregado en el MCU (REC-22..24). Seguro desde ISR una vez que el
// contador existe: la PRIMERA llamada debe ocurrir en contexto de tarea
// (registra el símbolo); desde ISR un contador desconocido se pierde
// contabilizado.
void count(const char* name, uint32_t n = 1);

// Marca puntual con argumento (FEAT-23, §7.1). Legal desde ISR (FW-05).
void event(const char* name, uint32_t arg = 0);

// Transición de máquina de estados (§7.8, FEAT-24). El estado se interna
// con parent = máquina (CAT-04). Solo desde tarea.
void state(const char* machine, const char* st);

// LED de salud (§7.8, FEAT-25). Niveles fijados por la spec.
enum StatusLevel : uint8_t {
    STATUS_OFF = 0,
    STATUS_GREEN = 1,
    STATUS_YELLOW = 2,
    STATUS_RED = 3,
};
void status(const char* name, StatusLevel level);

// Estadísticas internas (PR-13). Snapshot consistente para tests y REC_STATS.
struct Stats {
    uint32_t enqueued = 0;
    uint32_t dropped = 0;
    uint16_t dropped_by_kind[8] = {};
    uint16_t ring_high_water = 0;
    uint16_t sym_overflow = 0;
    uint32_t tx_bytes = 0;
};

Stats stats();

}  // namespace mole

#else  // !MOLE_ENABLED — FW-12: todo compila a nada, sin strings ni símbolos
// (Level/SymId/Stats vienen de mole_log.h, que se incluye siempre; acá solo
// los stubs inline vacíos. Con -O, los literales de los argumentos no
// referenciados desaparecen del binario.)

namespace mole {

struct Stats {
    uint32_t enqueued = 0;
    uint32_t dropped = 0;
    uint16_t dropped_by_kind[8] = {};
    uint16_t ring_high_water = 0;
    uint16_t sym_overflow = 0;
    uint32_t tx_bytes = 0;
};

inline Stats stats() { return {}; }
inline SymId intern(const char*, SymKind, SymId = 0) { return 0; }
inline void logs(Level, const char*) {}
inline void set_min_level(Level) {}
inline void set_tag_level(SymId, Level) {}
template <class... A>
inline void watch(A&&...) {}
template <class... A>
inline void watchEvery(A&&...) {}
inline void count(const char*, uint32_t = 1) {}
inline void event(const char*, uint32_t = 0) {}
inline void state(const char*, const char*) {}
enum StatusLevel : uint8_t {
    STATUS_OFF = 0,
    STATUS_GREEN = 1,
    STATUS_YELLOW = 2,
    STATUS_RED = 3,
};
inline void status(const char*, StatusLevel) {}

class ScopedSpan {
  public:
    explicit ScopedSpan(SymId) {}
};
template <class... A>
inline void command(A&&...) {}

}  // namespace mole

#endif
