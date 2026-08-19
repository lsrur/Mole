// SPDX-License-Identifier: MIT
// mole_core.cpp — intern de símbolos (CAT-01..06), cola de definiciones y
// catalog_hash (CAT-08). Camino frío: cada sitio pasa por acá una vez.

#include "mole.h"

#if MOLE_ENABLED

#include <atomic>
#include <cstring>
#include <mutex>
#include <new>

#include "mole_codec.h"
#include "mole_internal.h"
#include "mole_port.h"
#include "mole_ring.h"

namespace mole {
namespace {

struct SymEntry {
    const char* name;
    uint8_t kind;
    uint16_t parent;
};

struct Core {
    // ---- símbolos (CAT-01..06) ----
    SymEntry syms[MOLE_MAX_SYMBOLS];
    uint16_t sym_count = 0;  // ids 1..sym_count; 0 = kSymNone (CAT-06)
    uint16_t sym_overflow = 0;

    // ---- cola de definiciones (drenada por moleTask antes que los rings) ----
    // Entrada: [total u16][type u8][t_us u64][payload]
    uint8_t meta[detail::kMetaFifoSize];
    size_t meta_head = 0;  // lectura
    size_t meta_tail = 0;  // escritura
    size_t meta_used = 0;
    uint32_t meta_dropped = 0;

    // ---- catalog_hash (CAT-08): CRC32 incremental sobre payloads ----
    uint32_t hash_state = 0xFFFFFFFFu;

    std::mutex mtx;  // todo lo de arriba es camino frío

    // ---- rings (FW-04/05) — hot path, sin locks ----
    detail::Ring rings[MOLE_MAX_PRODUCERS];
    detail::Ring isr_ring;
    std::atomic_flag isr_lock = ATOMIC_FLAG_INIT;  // sección crítica corta

    // política por canal (PR-12); u8 atómico: CTL_SET_POLICY escribe en vivo
    std::atomic<uint8_t> policies[8];

    // contadores del productor (PR-13); atómicos relajados, se leen en stats
    std::atomic<uint32_t> enqueued{0};
    std::atomic<uint32_t> dropped{0};
    std::atomic<uint16_t> dropped_by_kind[8];

    // decimación: contador por canal para descartar 1 de cada N bajo presión
    std::atomic<uint32_t> decim_seq[8];

    // ---- watch rate limit (REC-09): último emit en ms por símbolo ----
    std::atomic<uint32_t> watch_last_ms[MOLE_MAX_SYMBOLS + 1];

    // ---- counters (REC-22..24): slots con delta atómico. La clave del
    // camino caliente es el PUNTERO del literal: identidad sin intern, y por
    // eso seguro desde ISR una vez registrado. ----
    static constexpr size_t kMaxCounters = 32;
    struct CounterSlot {
        std::atomic<const char*> name{nullptr};
        std::atomic<uint16_t> sym{0};
        std::atomic<uint32_t> delta{0};
    };
    CounterSlot counters[kMaxCounters];
    std::atomic<uint32_t> counter_lost_isr{0};

    // ---- spans (REC-18..21): contador global + stack por productor ----
    std::atomic<uint16_t> span_next{1};
    uint16_t span_stack[MOLE_MAX_PRODUCERS][8] = {};
    uint8_t span_depth[MOLE_MAX_PRODUCERS] = {};

    // ---- comandos (REC-29..31) ----
    static constexpr size_t kMaxCommands = 32;
    struct CmdSlot {
        const char* label = nullptr;
        uint16_t sym = 0;
        uint8_t arg_type = 0;  // 0 = sin argumento (draft.10)
        void (*fn0)() = nullptr;
        void (*fn_i32)(int32_t) = nullptr;
        void (*fn_f32)(float) = nullptr;
        int32_t min_i = 0, max_i = 0;
        float min_f = 0, max_f = 0;
    };
    CmdSlot cmds[kMaxCommands];
    uint16_t cmd_count = 0;

    // ---- log diferido (REC-34) y niveles (PR-19) ----
    std::atomic<uint16_t> fmt_next{1};
    // retención para el resync (CAT-10): lo justo para reconstruir el
    // REC_FMT_DEF (punteros a literales, no copias)
    struct FmtSlot {
        const char* fmt = nullptr;
        const char* file = nullptr;
        uint16_t line = 0;
        uint8_t argc = 0;
        uint8_t tags_len = 0;
        uint8_t tags[12] = {};
    };
    FmtSlot fmt_slots[MOLE_MAX_FMTS];
    // re-emisión de TYPE/ENUM defs (los descriptores viven en templates)
    void (*type_reemits[MOLE_MAX_TYPE_DEFS])() = {};
    std::atomic<uint16_t> type_reemit_n{0};
    std::atomic<uint8_t> min_level{0};
    // nivel por tag; índice = sym_id (0 = sin tag). u8 atómico, escrito por
    // CTL_SET_LEVEL, leído en el hot path.
    std::atomic<uint8_t> tag_level[MOLE_MAX_SYMBOLS + 1];

    Core() {
        // defaults de PR-12: DropNewest para todo; Block/Decimate se piden
        for (auto& p : policies) p.store(static_cast<uint8_t>(detail::Policy::DropNewest));
        for (auto& d : dropped_by_kind) d.store(0);
        for (auto& d : decim_seq) d.store(0);
        for (auto& t : tag_level) t.store(0);
        for (auto& w : watch_last_ms) w.store(0);
    }
};

Core& core() {
    static Core c;
    return c;
}

constexpr size_t kMetaEntryHeader = 2 + 1 + 8;

void meta_write_bytes(Core& c, const uint8_t* src, size_t n) {
    for (size_t i = 0; i < n; i++) {
        c.meta[c.meta_tail] = src[i];
        c.meta_tail = (c.meta_tail + 1) % detail::kMetaFifoSize;
    }
    c.meta_used += n;
}

void meta_read_bytes(Core& c, uint8_t* dst, size_t n) {
    for (size_t i = 0; i < n; i++) {
        dst[i] = c.meta[c.meta_head];
        c.meta_head = (c.meta_head + 1) % detail::kMetaFifoSize;
    }
    c.meta_used -= n;
}

}  // namespace

namespace detail {

static bool meta_push_impl(uint8_t type, const uint8_t* payload, uint8_t len,
                           bool feed_hash) {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    const size_t total = kMetaEntryHeader + len;
    if (c.meta_used + total > kMetaFifoSize) {
        // La definición se pierde acá pero el hash de abajo NO la incluye:
        // en el próximo handshake los hashes difieren y el resync (CAT-10)
        // re-emite el catálogo completo. Pérdida contabilizada, no silenciosa.
        c.meta_dropped++;
        return false;
    }
    const uint16_t total16 = static_cast<uint16_t>(total);
    uint8_t hdr[kMetaEntryHeader];
    put_u16(hdr, total16);
    hdr[2] = type;
    put_u64(hdr + 3, port::now_us());
    meta_write_bytes(c, hdr, sizeof hdr);
    meta_write_bytes(c, payload, len);
    if (feed_hash) {
        c.hash_state = crc32_update(c.hash_state, payload, len);
    }
    return true;
}

bool meta_push(uint8_t type, const uint8_t* payload, uint8_t len) {
    return meta_push_impl(type, payload, len, true);
}

bool meta_push_no_hash(uint8_t type, const uint8_t* payload, uint8_t len) {
    return meta_push_impl(type, payload, len, false);
}

bool meta_pop(MetaView* out, uint8_t* buf) {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    if (c.meta_used == 0) return false;
    uint8_t hdr[kMetaEntryHeader];
    meta_read_bytes(c, hdr, sizeof hdr);
    const uint16_t total = get_u16(hdr);
    const uint8_t len = static_cast<uint8_t>(total - kMetaEntryHeader);
    meta_read_bytes(c, buf, len);
    out->type = hdr[2];
    out->t_us = get_u64(hdr + 3);
    out->payload = buf;
    out->len = len;
    return true;
}

uint32_t meta_dropped() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    return c.meta_dropped;
}

uint32_t catalog_hash() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    return c.hash_state ^ 0xFFFFFFFFu;
}

uint16_t sym_overflow_count() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    return c.sym_overflow;
}

void reset_for_tests() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    c.sym_count = 0;
    c.sym_overflow = 0;
    c.meta_head = c.meta_tail = c.meta_used = 0;
    c.meta_dropped = 0;
    c.hash_state = 0xFFFFFFFFu;
    for (auto& r : c.rings) r.reset();
    c.isr_ring.reset();
    for (auto& s : c.counters) {
        s.name.store(nullptr);
        s.sym.store(0);
        s.delta.store(0);
    }
    c.counter_lost_isr.store(0);
    // los type_id<T>() son estáticos de proceso: sus re-emisiones quedarían
    // inconsistentes con una tabla de símbolos vaciada (solo pasa en tests)
    c.type_reemit_n.store(0);
    for (auto& s : c.fmt_slots) s = Core::FmtSlot{};
    c.fmt_next.store(1);
    c.span_next.store(1);
    for (auto& d : c.span_depth) d = 0;
    for (auto& s : c.cmds) s = Core::CmdSlot{};
    c.cmd_count = 0;
    for (auto& p : c.policies) p.store(static_cast<uint8_t>(Policy::DropNewest));
    c.enqueued.store(0);
    c.dropped.store(0);
    for (auto& d : c.dropped_by_kind) d.store(0);
    for (auto& d : c.decim_seq) d.store(0);
}

// ---------------------------------------------------------------------------
// Rings y políticas
// ---------------------------------------------------------------------------

Channel channel_of(uint8_t rec_type) {
    switch (rec_type) {
        case REC_LOG:
        case REC_LOG_FMT:
            return CH_LOG;
        case REC_WATCH:
        case REC_WATCH_STR:
            return CH_WATCH;
        case REC_SPAN_BEGIN:
        case REC_SPAN_END:
        case REC_SPAN_ABORT:
            return CH_SPAN;
        case REC_COUNTER:
            return CH_COUNTER;
        case REC_DUMP:
            return CH_DUMP;
        case REC_EVENT:
            return CH_EVENT;
        case REC_STATE:
        case REC_STATUS:
            return CH_STATE;
        default:
            return CH_OTHER;
    }
}

void set_policy(Channel ch, Policy p) {
    core().policies[ch].store(static_cast<uint8_t>(p), std::memory_order_relaxed);
}

Policy policy_of(Channel ch) {
    return static_cast<Policy>(core().policies[ch].load(std::memory_order_relaxed));
}

namespace {

Ring& slot_ring(Core& c, uint8_t slot) {
    Ring& r = c.rings[slot];
    if (!r.valid()) {
        // primera vez que esta tarea produce: asignación fuera del hot path
        // (una vez en la vida de la tarea, FW-04)
        r.init(new uint8_t[MOLE_RING_SIZE], MOLE_RING_SIZE);
    }
    return r;
}

void count_drop(Core& c, Channel ch) {
    c.dropped.fetch_add(1, std::memory_order_relaxed);
    c.dropped_by_kind[ch].fetch_add(1, std::memory_order_relaxed);
}

// Decimación creciente bajo presión (PR-12): con el ring a más de 3/4,
// descarta 1 de cada 2; a más de 7/8, 3 de cada 4.
bool decimate_drops(Core& c, Channel ch, const Ring& r) {
    const uint32_t used = r.used();
    if (used * 4 < r.size() * 3) return false;
    const uint32_t seq = c.decim_seq[ch].fetch_add(1, std::memory_order_relaxed);
    if (used * 8 >= r.size() * 7) return (seq & 3) != 3;  // pasa 1 de cada 4
    return (seq & 1) != 1;                                // pasa 1 de cada 2
}

}  // namespace

bool ring_push(uint8_t rec_type, const uint8_t* payload, uint8_t len) {
    Core& c = core();
    const uint8_t slot = port::task_slot();
    const Channel ch = channel_of(rec_type);
    if (slot >= MOLE_MAX_PRODUCERS) {
        count_drop(c, ch);
        return false;
    }
    Ring& r = slot_ring(c, slot);
    const Policy pol = policy_of(ch);

    if (pol == Policy::Decimate && decimate_drops(c, ch, r)) {
        count_drop(c, ch);
        return false;
    }

    const uint64_t t = port::now_us();
    if (r.push(rec_type, t, payload, len)) {
        c.enqueued.fetch_add(1, std::memory_order_relaxed);
        return true;
    }

    if (pol == Policy::Block) {
        // FW-08: nunca más de MOLE_BLOCK_TIMEOUT_MS; después descarta.
        const uint64_t deadline = t + static_cast<uint64_t>(MOLE_BLOCK_TIMEOUT_MS) * 1000;
        while (port::now_us() < deadline) {
            port::yield_short();
            if (r.push(rec_type, port::now_us(), payload, len)) {
                c.enqueued.fetch_add(1, std::memory_order_relaxed);
                return true;
            }
        }
    }
    // DropNewest / DropOldest degradado / Block vencido / Decimate lleno
    count_drop(c, ch);
    return false;
}

bool isr_push(uint8_t rec_type, const uint8_t* payload, uint8_t len) {
    Core& c = core();
    if (!c.isr_ring.valid()) {
        // el ring de ISR se crea en begin(); si no existe, contabilizar
        count_drop(c, channel_of(rec_type));
        return false;
    }
    // sección crítica corta: en el target esto es portENTER_CRITICAL_ISR
    while (c.isr_lock.test_and_set(std::memory_order_acquire)) {
    }
    const bool ok = c.isr_ring.push(rec_type, port::now_us(), payload, len);
    c.isr_lock.clear(std::memory_order_release);
    if (ok) {
        c.enqueued.fetch_add(1, std::memory_order_relaxed);
    } else {
        count_drop(c, channel_of(rec_type));
    }
    return ok;
}

// El ring de ISR se aloca acá para poder testear sin begin() completo.
void ensure_isr_ring() {
    Core& c = core();
    if (!c.isr_ring.valid()) {
        c.isr_ring.init(new uint8_t[MOLE_ISR_RING_SIZE], MOLE_ISR_RING_SIZE);
    }
}

bool ring_pop_slot(uint8_t slot, uint8_t* rec_type, uint64_t* t_us,
                   uint8_t* buf, uint8_t* len) {
    Core& c = core();
    if (slot >= MOLE_MAX_PRODUCERS || !c.rings[slot].valid()) return false;
    RingView v;
    if (!c.rings[slot].pop(&v, buf)) return false;
    *rec_type = v.type;
    *t_us = v.t_us;
    *len = v.len;
    return true;
}

bool isr_pop(uint8_t* rec_type, uint64_t* t_us, uint8_t* buf, uint8_t* len) {
    Core& c = core();
    if (!c.isr_ring.valid()) return false;
    while (c.isr_lock.test_and_set(std::memory_order_acquire)) {
    }
    RingView v;
    const bool ok = c.isr_ring.pop(&v, buf);
    c.isr_lock.clear(std::memory_order_release);
    if (!ok) return false;
    *rec_type = v.type;
    *t_us = v.t_us;
    *len = v.len;
    return true;
}

RingStats ring_stats() {
    Core& c = core();
    RingStats s;
    s.enqueued = c.enqueued.load(std::memory_order_relaxed);
    s.dropped = c.dropped.load(std::memory_order_relaxed);
    for (int i = 0; i < 8; i++) {
        s.dropped_by_kind[i] = c.dropped_by_kind[i].load(std::memory_order_relaxed);
    }
    uint16_t hw = 0;
    for (const auto& r : c.rings) {
        if (r.valid() && r.high_water() > hw) hw = r.high_water();
    }
    if (c.isr_ring.valid() && c.isr_ring.high_water() > hw) hw = c.isr_ring.high_water();
    s.ring_high_water = hw;
    return s;
}

}  // namespace detail

SymId intern(const char* name, SymKind kind, SymId parent) {
    Core& c = core();
    uint8_t payload[255];
    uint8_t plen = 0;
    SymId id;
    {
        std::lock_guard<std::mutex> lock(c.mtx);

        // dedup: primero identidad de puntero (literales fusionados), después
        // contenido. kind y parent participan de la identidad (CAT-04).
        for (uint16_t i = 0; i < c.sym_count; i++) {
            const SymEntry& e = c.syms[i];
            if (e.kind != static_cast<uint8_t>(kind) || e.parent != parent) continue;
            if (e.name == name || std::strcmp(e.name, name) == 0) {
                return static_cast<SymId>(i + 1);
            }
        }

        if (c.sym_count >= MOLE_MAX_SYMBOLS) {
            // CAT-05: overflow contabilizado; NUNCA se reintenta en loop
            // (bug de v1: registry lleno ⇒ reenvío en cada llamada).
            c.sym_overflow++;
            return kSymOverflow;
        }

        id = static_cast<SymId>(c.sym_count + 1);
        c.syms[c.sym_count] = SymEntry{name, static_cast<uint8_t>(kind), parent};
        c.sym_count++;

        // REC_SYM_DEF — CAT-04, nombre con prefijo u8 (PR-20)
        size_t name_len = std::strlen(name);
        if (name_len > 255 - 6) name_len = 255 - 6;
        put_u16(payload, id);
        payload[2] = static_cast<uint8_t>(kind);
        put_u16(payload + 3, parent);
        payload[5] = static_cast<uint8_t>(name_len);
        std::memcpy(payload + 6, name, name_len);
        plen = static_cast<uint8_t>(6 + name_len);
    }
    // fuera del lock: meta_push toma el mismo mutex
    detail::meta_push(REC_SYM_DEF, payload, plen);
    return id;
}

Stats stats() {
    Core& c = core();
    const detail::RingStats rs = detail::ring_stats();
    Stats s;
    s.enqueued = rs.enqueued;
    s.dropped = rs.dropped;
    for (int i = 0; i < 8; i++) s.dropped_by_kind[i] = rs.dropped_by_kind[i];
    s.ring_high_water = rs.ring_high_water;
    {
        std::lock_guard<std::mutex> lock(c.mtx);
        s.sym_overflow = c.sym_overflow;
    }
    return s;
}

// ---------------------------------------------------------------------------
// Log diferido (F1-T04)
// ---------------------------------------------------------------------------

namespace detail {

bool log_enabled(uint8_t level, SymId tag) {
    Core& c = core();
    if (level < c.min_level.load(std::memory_order_relaxed)) return false;
    if (tag != 0 && tag <= MOLE_MAX_SYMBOLS &&
        level < c.tag_level[tag].load(std::memory_order_relaxed)) {
        return false;
    }
    // MOLE_LEVEL_MIN (FW-13) filtra en compilación en las macros de F2;
    // acá vale el filtro de runtime.
    return true;
}

namespace {

// payload REC-34: fmt_id, file_sym, line, argc, arg_types, fmt (PR-20)
uint8_t build_fmt_payload(uint8_t* payload, uint16_t id, uint16_t file_sym,
                          uint16_t line, uint8_t argc, const uint8_t* tags,
                          uint8_t tags_len, const char* fmt) {
    put_u16(payload, id);
    put_u16(payload + 2, file_sym);
    put_u16(payload + 4, line);
    payload[6] = argc;
    std::memcpy(payload + 7, tags, tags_len);
    size_t fmt_len = std::strlen(fmt);
    const size_t max_fmt = 255 - (7 + tags_len + 1);
    if (fmt_len > max_fmt) fmt_len = max_fmt;
    payload[7 + tags_len] = static_cast<uint8_t>(fmt_len);
    std::memcpy(payload + 7 + tags_len + 1, fmt, fmt_len);
    return static_cast<uint8_t>(7 + tags_len + 1 + fmt_len);
}

}  // namespace

uint16_t register_fmt(const char* fmt, const char* file, uint16_t line,
                      uint8_t argc, const uint8_t* tags, uint8_t tags_len) {
    Core& c = core();
#if MOLE_LOG_SOURCE
    const SymId file_sym = intern(file, KIND_FILE, 0);
#else
    (void)file;
    const SymId file_sym = 0;
#endif
    const uint16_t id = c.fmt_next.fetch_add(1, std::memory_order_relaxed);

    // retención para el resync (CAT-10): punteros a literales, sin copia
    if (id <= MOLE_MAX_FMTS && tags_len <= sizeof c.fmt_slots[0].tags) {
        Core::FmtSlot& s = c.fmt_slots[id - 1];
        s.fmt = fmt;
        s.file = file;
        s.line = line;
        s.argc = argc;
        s.tags_len = tags_len;
        std::memcpy(s.tags, tags, tags_len);
    }

    uint8_t payload[255];
    const uint8_t plen =
        build_fmt_payload(payload, id, file_sym, line, argc, tags, tags_len, fmt);
    meta_push(REC_FMT_DEF, payload, plen);
    return id;
}

// ---- accesores del resync (CAT-10) ----

uint16_t sym_table_count() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    return c.sym_count;
}

bool sym_table_entry(uint16_t idx, uint8_t* payload, uint8_t* len) {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    if (idx >= c.sym_count) return false;
    const SymEntry& e = c.syms[idx];
    size_t name_len = std::strlen(e.name);
    if (name_len > 255 - 6) name_len = 255 - 6;
    put_u16(payload, static_cast<uint16_t>(idx + 1));
    payload[2] = e.kind;
    put_u16(payload + 3, e.parent);
    payload[5] = static_cast<uint8_t>(name_len);
    std::memcpy(payload + 6, e.name, name_len);
    *len = static_cast<uint8_t>(6 + name_len);
    return true;
}

uint16_t fmt_table_count() {
    Core& c = core();
    const uint16_t next = c.fmt_next.load(std::memory_order_relaxed);
    const uint16_t used = static_cast<uint16_t>(next - 1);
    return used > MOLE_MAX_FMTS ? MOLE_MAX_FMTS : used;
}

bool fmt_table_entry(uint16_t idx, uint8_t* payload, uint8_t* len) {
    Core& c = core();
    if (idx >= fmt_table_count()) return false;
    const Core::FmtSlot& s = c.fmt_slots[idx];
    if (s.fmt == nullptr) return false;
#if MOLE_LOG_SOURCE
    const SymId file_sym = intern(s.file, KIND_FILE, 0);  // dedup: mismo id
#else
    const SymId file_sym = 0;
#endif
    *len = build_fmt_payload(payload, static_cast<uint16_t>(idx + 1), file_sym,
                             s.line, s.argc, s.tags, s.tags_len, s.fmt);
    return true;
}

void register_type_reemit(void (*fn)()) {
    Core& c = core();
    const uint16_t i = c.type_reemit_n.fetch_add(1, std::memory_order_relaxed);
    if (i < MOLE_MAX_TYPE_DEFS) {
        c.type_reemits[i] = fn;
    }
}

size_t type_reemit_count() {
    Core& c = core();
    const uint16_t n = c.type_reemit_n.load(std::memory_order_relaxed);
    return n > MOLE_MAX_TYPE_DEFS ? MOLE_MAX_TYPE_DEFS : n;
}

void run_type_reemit(size_t i) {
    Core& c = core();
    if (i < type_reemit_count() && c.type_reemits[i]) {
        c.type_reemits[i]();
    }
}

void count_oversize_log() {
    count_drop(core(), CH_LOG);
}

// ---- registro de tipos (mole_describe.h) ----

uint16_t alloc_type_id() {
    static std::atomic<uint16_t> next{1};
    return next.fetch_add(1, std::memory_order_relaxed);
}

bool meta_push_public(uint8_t type, const uint8_t* payload, uint8_t len) {
    return meta_push(type, payload, len);
}

bool meta_push_no_hash_public(uint8_t type, const uint8_t* payload, uint8_t len) {
    return meta_push_no_hash(type, payload, len);
}

}  // namespace detail

// ---------------------------------------------------------------------------
// Watch / counters / eventos (F1-T06)
// ---------------------------------------------------------------------------

namespace detail {

bool watch_emit(SymId sym, uint8_t wire, const void* bytes, uint8_t n) {
    uint8_t payload[2 + 1 + 8];
    put_u16(payload, sym);
    payload[2] = wire;
    std::memcpy(payload + 3, bytes, n);
    return ring_push(REC_WATCH, payload, static_cast<uint8_t>(3 + n));
}

bool watch_emit_str(SymId sym, const char* s) {
    uint8_t payload[255];
    put_u16(payload, sym);
    size_t n = s ? std::strlen(s) : 0;
    if (n > 250) n = 250;
    payload[2] = static_cast<uint8_t>(n);
    std::memcpy(payload + 3, s, n);
    return ring_push(REC_WATCH_STR, payload, static_cast<uint8_t>(3 + n));
}

bool watch_rate_ok(SymId sym, uint32_t every_ms) {
    if (sym > MOLE_MAX_SYMBOLS) return false;
    Core& c = core();
    const uint32_t now_ms = static_cast<uint32_t>(port::now_us() / 1000);
    const uint32_t last = c.watch_last_ms[sym].load(std::memory_order_relaxed);
    if (last != 0 && now_ms - last < every_ms) return false;
    // carrera benigna: dos hilos podrían pasar juntos una vez por ventana
    c.watch_last_ms[sym].store(now_ms == 0 ? 1 : now_ms, std::memory_order_relaxed);
    return true;
}

uint8_t counters_collect(uint8_t* payload) {
    Core& c = core();
    uint8_t n = 0;
    uint8_t p = 1;  // payload[0] = n, al final
    for (auto& slot : c.counters) {
        const uint16_t sym = slot.sym.load(std::memory_order_relaxed);
        if (sym == 0) continue;
        const uint32_t delta = slot.delta.exchange(0, std::memory_order_relaxed);
        if (delta == 0) continue;
        put_u16(payload + p, sym);
        put_u32(payload + p + 2, delta);
        p = static_cast<uint8_t>(p + 6);
        n++;
        if (p + 6 > 255) break;
    }
    if (n == 0) return 0;
    payload[0] = n;
    return p;
}

uint32_t counter_lost_from_isr() {
    return core().counter_lost_isr.load(std::memory_order_relaxed);
}

}  // namespace detail

void count(const char* name, uint32_t n) {
    Core& c = core();
    // camino caliente: identidad de puntero sobre ≤32 slots, sin locks
    for (auto& slot : c.counters) {
        if (slot.name.load(std::memory_order_acquire) == name) {
            slot.delta.fetch_add(n, std::memory_order_relaxed);
            return;
        }
    }
    if (port::in_isr()) {
        // registrar exige intern (mutex): el contrato es primera llamada en
        // contexto de tarea. Desde ISR, un contador desconocido se pierde
        // CONTABILIZADO, nunca en silencio.
        c.counter_lost_isr.fetch_add(n, std::memory_order_relaxed);
        return;
    }
    // cold path: registrar (una vez por contador)
    const SymId sym = intern(name, KIND_COUNTER);
    if (sym == kSymOverflow) return;
    for (auto& slot : c.counters) {
        const char* expected = nullptr;
        if (slot.name.compare_exchange_strong(expected, name,
                                              std::memory_order_acq_rel)) {
            slot.sym.store(sym, std::memory_order_relaxed);
            slot.delta.fetch_add(n, std::memory_order_relaxed);
            return;
        }
        // otro hilo pudo registrar el mismo literal
        if (expected == name) {
            slot.delta.fetch_add(n, std::memory_order_relaxed);
            return;
        }
    }
    // sin slots libres: pérdida contabilizada en el canal counter
    detail::count_drop(c, detail::CH_COUNTER);
}

void event(const char* name, uint32_t arg) {
    if (port::in_isr()) {
        // FW-05 permite event() desde ISR, pero el nombre debe estar ya
        // internado (el intern toma mutex). F1: registro previo en tarea o
        // pérdida contabilizada. Se revisa con el uso real.
        core().counter_lost_isr.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    const SymId sym = intern(name, static_cast<SymKind>(0));
    if (sym == kSymOverflow) return;
    uint8_t payload[6];
    put_u16(payload, sym);
    put_u32(payload + 2, arg);
    detail::ring_push(REC_EVENT, payload, 6);
}

// ---------------------------------------------------------------------------
// Spans (REC-18..21)
// ---------------------------------------------------------------------------

namespace detail {

uint16_t span_begin(SymId sym) {
    Core& c = core();
    // contador GLOBAL de instancias (REC-21, draft.9); 0 se saltea
    uint16_t id = c.span_next.fetch_add(1, std::memory_order_relaxed);
    if (id == 0) id = c.span_next.fetch_add(1, std::memory_order_relaxed);
    const uint8_t slot = port::task_slot();
    uint16_t parent = 0;
    if (slot < MOLE_MAX_PRODUCERS) {
        const uint8_t d = c.span_depth[slot];
        if (d > 0 && d <= 8) parent = c.span_stack[slot][d - 1];
        if (d < 8) {
            c.span_stack[slot][d] = id;
            c.span_depth[slot] = d + 1;
        }
    }
    uint8_t payload[7];
    put_u16(payload, id);
    put_u16(payload + 2, sym);
    put_u16(payload + 4, parent);
    payload[6] = slot;
    ring_push(REC_SPAN_BEGIN, payload, 7);
    return id;
}

void span_end(uint16_t span_id) {
    Core& c = core();
    const uint8_t slot = port::task_slot();
    if (slot < MOLE_MAX_PRODUCERS && c.span_depth[slot] > 0) {
        c.span_depth[slot]--;
    }
    uint8_t payload[2];
    put_u16(payload, span_id);
    ring_push(REC_SPAN_END, payload, 2);
}

}  // namespace detail

// ---------------------------------------------------------------------------
// Comandos (REC-29..31)
// ---------------------------------------------------------------------------

namespace {

uint16_t command_register(Core& c, const char* label, uint8_t arg_type) {
    std::lock_guard<std::mutex> lock(c.mtx);
    if (c.cmd_count >= Core::kMaxCommands) return 0;
    const uint16_t id = c.cmd_count + 1;
    Core::CmdSlot& s = c.cmds[c.cmd_count];
    s.label = label;
    s.arg_type = arg_type;
    c.cmd_count++;
    return id;
}

// payload REC_CMD_DEF (draft.10): cmd_id, sym, arg_type, min[], max[]
uint8_t build_cmd_payload(const Core::CmdSlot& s, uint16_t id, uint8_t* payload) {
    put_u16(payload, id);
    put_u16(payload + 2, s.sym);
    payload[4] = s.arg_type;
    uint8_t p = 5;
    if (s.arg_type == WIRE_I32) {
        put_u32(payload + p, static_cast<uint32_t>(s.min_i));
        put_u32(payload + p + 4, static_cast<uint32_t>(s.max_i));
        p += 8;
    } else if (s.arg_type == WIRE_F32) {
        std::memcpy(payload + p, &s.min_f, 4);
        std::memcpy(payload + p + 4, &s.max_f, 4);
        p += 8;
    }
    return p;
}

void command_emit_def(Core& c, uint16_t id) {
    uint8_t payload[16];
    const uint8_t p = build_cmd_payload(c.cmds[id - 1], id, payload);
    // no alimenta el catalog_hash (CAT-08 no lista CMD_DEF)
    detail::meta_push_no_hash(REC_CMD_DEF, payload, p);
}

}  // namespace

void command(const char* label, void (*fn)()) {
    Core& c = core();
    const uint16_t id = command_register(c, label, 0);
    if (id == 0) return;
    c.cmds[id - 1].fn0 = fn;
    c.cmds[id - 1].sym = intern(label, KIND_COMMAND);
    command_emit_def(c, id);
}

void command(const char* label, void (*fn)(int32_t), int32_t min, int32_t max) {
    Core& c = core();
    const uint16_t id = command_register(c, label, WIRE_I32);
    if (id == 0) return;
    Core::CmdSlot& s = c.cmds[id - 1];
    s.fn_i32 = fn;
    s.min_i = min;
    s.max_i = max;
    s.sym = intern(label, KIND_COMMAND);
    command_emit_def(c, id);
}

void command(const char* label, void (*fn)(float), float min, float max) {
    Core& c = core();
    const uint16_t id = command_register(c, label, WIRE_F32);
    if (id == 0) return;
    Core::CmdSlot& s = c.cmds[id - 1];
    s.fn_f32 = fn;
    s.min_f = min;
    s.max_f = max;
    s.sym = intern(label, KIND_COMMAND);
    command_emit_def(c, id);
}

namespace detail {

// Despacho de CTL_CMD (REC-31): corre en la moleTask (PR-17).
bool command_dispatch(uint16_t cmd_id, uint8_t arg_type, const uint8_t* arg,
                      uint8_t arg_len) {
    Core& c = core();
    if (cmd_id == 0 || cmd_id > c.cmd_count) return false;
    const Core::CmdSlot& s = c.cmds[cmd_id - 1];
    if (arg_type != s.arg_type) return false;
    switch (s.arg_type) {
        case 0:
            if (s.fn0) s.fn0();
            return true;
        case WIRE_I32: {
            if (arg_len < 4 || !s.fn_i32) return false;
            int32_t v = static_cast<int32_t>(get_u32(arg));
            if (v < s.min_i) v = s.min_i;
            if (v > s.max_i) v = s.max_i;
            s.fn_i32(v);
            return true;
        }
        case WIRE_F32: {
            if (arg_len < 4 || !s.fn_f32) return false;
            float v;
            std::memcpy(&v, arg, 4);
            if (v < s.min_f) v = s.min_f;
            if (v > s.max_f) v = s.max_f;
            s.fn_f32(v);
            return true;
        }
        default:
            return false;
    }
}

uint16_t cmd_table_count() {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    return c.cmd_count;
}

bool cmd_table_entry(uint16_t idx, uint8_t* payload, uint8_t* len) {
    Core& c = core();
    std::lock_guard<std::mutex> lock(c.mtx);
    if (idx >= c.cmd_count) return false;
    *len = build_cmd_payload(c.cmds[idx], static_cast<uint16_t>(idx + 1), payload);
    return true;
}

}  // namespace detail

void set_min_level(Level lvl) {
    core().min_level.store(static_cast<uint8_t>(lvl), std::memory_order_relaxed);
}

void set_tag_level(SymId tag, Level lvl) {
    if (tag == 0 || tag > MOLE_MAX_SYMBOLS) return;
    core().tag_level[tag].store(static_cast<uint8_t>(lvl), std::memory_order_relaxed);
}

void logs(Level lvl, const char* msg) {
    if (!detail::log_enabled(static_cast<uint8_t>(lvl), 0)) return;
    // REC-36: {level, task, core, tag_sym, file_sym, line, msg} — sin las
    // macros no hay sitio, así que tag/file/line van en cero.
    uint8_t payload[255];
    payload[0] = static_cast<uint8_t>(lvl);
    payload[1] = port::task_slot();
    payload[2] = port::core_id();
    put_u16(payload + 3, 0);
    put_u16(payload + 5, 0);
    put_u16(payload + 7, 0);
    size_t n = msg ? std::strlen(msg) : 0;
    if (n > 200) n = 200;  // REC-05
    payload[9] = static_cast<uint8_t>(n);
    std::memcpy(payload + 10, msg, n);
    detail::ring_push(REC_LOG, payload, static_cast<uint8_t>(10 + n));
}

}  // namespace mole

#endif  // MOLE_ENABLED
