// SPDX-License-Identifier: MIT
// mole.h — API pública de Mole v2 (spec §8.5). En F1: logs diferidos,
// descriptores, watch, counters, eventos. Sin instancia global con estado
// compartido (FW-15): funciones libres, el estado vive en el .cpp.
#pragma once

#include "mole_config.h"
#include "mole_wire.h"

#if MOLE_ENABLED

#include <stddef.h>
#include <stdint.h>

namespace mole {

// Niveles — REC-02.
enum class Level : uint8_t {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
};

using SymId = uint16_t;

// Registro de símbolos (CAT-01/02). El costo en régimen del hot path es el
// load de un static local que guarda el id ya resuelto.
// CONTRATO: `name` DEBE apuntar a memoria estable (un literal); no se copia.
// Las macros lo garantizan por construcción.
SymId intern(const char* name, SymKind kind, SymId parent = kSymNone);

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

#else  // !MOLE_ENABLED — FW-12: todo compila a nada

namespace mole {}

#endif
