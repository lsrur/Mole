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

    // ---- log diferido (REC-34) y niveles (PR-19) ----
    std::atomic<uint16_t> fmt_next{1};
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

bool meta_push(uint8_t type, const uint8_t* payload, uint8_t len) {
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
    c.hash_state = crc32_update(c.hash_state, payload, len);
    return true;
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

    // payload REC-34: fmt_id, file_sym, line, argc, arg_types, fmt (PR-20)
    uint8_t payload[255];
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
    meta_push(REC_FMT_DEF, payload, static_cast<uint8_t>(7 + tags_len + 1 + fmt_len));
    return id;
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
