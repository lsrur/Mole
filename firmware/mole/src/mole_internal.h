// SPDX-License-Identifier: MIT
// mole_internal.h — estado interno del core, compartido entre mole_core.cpp,
// mole_task.cpp y los tests de host. No es API pública.
#pragma once

#include <stddef.h>
#include <stdint.h>

#include "mole_config.h"
#include "mole_wire.h"

namespace mole {
namespace detail {

// Tamaño de la cola de definiciones de catálogo (camino frío: cada sitio
// pasa por acá una sola vez en la vida del proceso).
inline constexpr size_t kMetaFifoSize = 2048;

// Vista de una definición pendiente de emitir.
struct MetaView {
    uint8_t type = 0;
    uint64_t t_us = 0;
    const uint8_t* payload = nullptr;
    uint8_t len = 0;
};

// Encola una definición de catálogo (REC_SYM_DEF/FMT_DEF/TYPE_DEF/ENUM_DEF).
// Alimenta el catalog_hash (CAT-08) en orden de encolado. Si la cola está
// llena, contabiliza el descarte: el resync (CAT-10) es la red de seguridad.
bool meta_push(uint8_t type, const uint8_t* payload, uint8_t len);

// Saca la definición más vieja copiándola a buf (≥255 bytes). La moleTask
// drena esto ANTES que los rings, así toda definición viaja antes o junto
// con su primer uso.
bool meta_pop(MetaView* out, uint8_t* buf);

// Cantidad de definiciones descartadas por cola llena.
uint32_t meta_dropped();

// catalog_hash incremental (CAT-08).
uint32_t catalog_hash();

// Contador de overflow de símbolos (CAT-05).
uint16_t sym_overflow_count();

// Reinicio total del estado del core — SOLO para tests de host.
void reset_for_tests();

// ---------------------------------------------------------------------------
// Rings y políticas (FW-04/05/08, PR-12)
// ---------------------------------------------------------------------------

// Interpretación de los 8 slots de dropped_by_kind de REC_STATS (PR-13).
enum Channel : uint8_t {
    CH_LOG = 0,
    CH_WATCH = 1,
    CH_SPAN = 2,
    CH_COUNTER = 3,
    CH_DUMP = 4,
    CH_EVENT = 5,
    CH_STATE = 6,
    CH_OTHER = 7,  // meta, blobs, checks, ctrl
};

enum class Policy : uint8_t {
    Block = 0,
    DropNewest = 1,
    DropOldest = 2,  // en rings degrada a DropNewest; el "último valor"
                     // de watch (su caso real) se implementa en T-06
    Decimate = 3,
};

// Canal de un tipo de record (para política y contabilidad).
Channel channel_of(uint8_t rec_type);

// Política vigente de un canal (CTL_SET_POLICY la cambia en vivo).
void set_policy(Channel ch, Policy p);
Policy policy_of(Channel ch);

// Encola un record desde la tarea actual (slot implícito, FW-04) aplicando
// la política del canal. Devuelve false si se descartó (ya contabilizado).
bool ring_push(uint8_t rec_type, const uint8_t* payload, uint8_t len);

// Encola desde ISR (ring dedicado, FW-05). Nunca bloquea.
bool isr_push(uint8_t rec_type, const uint8_t* payload, uint8_t len);

// Crea el ring de ISR si no existe (lo llama begin(); expuesto para tests).
void ensure_isr_ring();

// Lado consumidor (moleTask): drena el ring del slot dado o el de ISR.
// buf debe tener ≥255 bytes.
bool ring_pop_slot(uint8_t slot, uint8_t* rec_type, uint64_t* t_us,
                   uint8_t* buf, uint8_t* len);
bool isr_pop(uint8_t* rec_type, uint64_t* t_us, uint8_t* buf, uint8_t* len);

// Contadores agregados (PR-13).
struct RingStats {
    uint32_t enqueued = 0;
    uint32_t dropped = 0;
    uint16_t dropped_by_kind[8] = {};
    uint16_t ring_high_water = 0;
};
RingStats ring_stats();

// ---------------------------------------------------------------------------
// Counters (REC-22..24): agregados en el MCU, batcheados por la moleTask
// ---------------------------------------------------------------------------

// Junta los deltas no-cero en un payload REC_COUNTER {n, entries} y los
// resetea. Devuelve el largo (0 = nada para emitir). La llama la moleTask
// cada 250 ms.
uint8_t counters_collect(uint8_t* payload);

// Incrementos perdidos por llamar count() desde ISR sin registro previo.
uint32_t counter_lost_from_isr();

// ---------------------------------------------------------------------------
// La bomba de la moleTask (FW-06, PR-05): portable y testeable en host.
// El wrapper de FreeRTOS y el transporte la llaman en loop.
// ---------------------------------------------------------------------------

// Recibe un frame de WIRE completo (COBS + delimitador 0x00).
using FrameSink = bool (*)(void* ctx, const uint8_t* wire, size_t len);

struct TaskState {
    uint8_t frame_buf[MOLE_FRAME_MAX];
    uint8_t wire_buf[MOLE_FRAME_MAX + MOLE_FRAME_MAX / 254 + 8];
    bool frame_open = false;
    uint64_t frame_t_base = 0;
    uint64_t frame_opened_at = 0;
    uint16_t seq = 0;
    uint16_t rec_count = 0;
    size_t frame_len = 0;
    uint8_t flags_pending = 0;   // FLAG_CATALOG/FLAG_DROPS del frame abierto
    bool urgent = false;         // PR-05(c)
    uint64_t last_stats_us = 0;
    uint64_t last_counter_us = 0;
    uint32_t last_dropped_seen = 0;
    uint32_t tx_bytes = 0;
    // ---- downlink (F1-T09) ----
    uint8_t rx_buf[1024];
    size_t rx_len = 0;
    bool hello_seen = false;        // SEC-03
    uint64_t last_session_us = 0;   // CAT-11: REC_SESSION cada 2 s sin HELLO
    uint32_t rx_bad_frames = 0;     // CRC/COBS malos, descartados (PR-16)
};

// Un paso: drena meta → counters/stats vencidos → rings (round-robin),
// arma frames y emite por el sink lo que corresponda cerrar (PR-05).
// Devuelve cuántos records movió en este paso.
uint32_t task_pump(TaskState& st, FrameSink sink, void* ctx);

// Cierra y emite el frame abierto, si lo hay (mole::flush()).
void task_flush(TaskState& st, FrameSink sink, void* ctx);

// ---------------------------------------------------------------------------
// Downlink (F1-T09): PR-15..17, CAT-09..11, SEC-02/03
// ---------------------------------------------------------------------------

// Alimenta bytes de downlink. El parseo y los efectos ocurren acá, en el
// contexto de la moleTask (PR-17). Frames inválidos se descartan en
// silencio y se cuentan (PR-16).
void downlink_feed(TaskState& st, FrameSink sink, void* ctx,
                   const uint8_t* data, size_t n);

// Como meta_push pero SIN alimentar el catalog_hash: para re-emisiones de
// resync (el hash cubre cada definición una sola vez; el host toma el hash
// de REC_SESSION, no lo recomputa del stream).
bool meta_push_no_hash(uint8_t type, const uint8_t* payload, uint8_t len);

// Registra la función de re-emisión de un TYPE/ENUM def (para el resync).
void register_type_reemit(void (*fn)());

// Acceso de solo lectura al catálogo para re-emitir en el resync.
uint16_t sym_table_count();
bool sym_table_entry(uint16_t idx, uint8_t* payload, uint8_t* len);  // payload REC_SYM_DEF
uint16_t fmt_table_count();
bool fmt_table_entry(uint16_t idx, uint8_t* payload, uint8_t* len);  // payload REC_FMT_DEF
size_t type_reemit_count();
void run_type_reemit(size_t i);

}  // namespace detail
}  // namespace mole
