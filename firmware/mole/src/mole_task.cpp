// SPDX-License-Identifier: MIT
// mole_task.cpp — la bomba de la moleTask (F1-T07): drenaje round-robin
// (FW-06), armado de frames con cierre por tamaño/tiempo/urgencia (PR-05,
// PR-08), REC_STATS (PR-13) y counters batcheados (REC-23).
//
// Esta parte es portable (host y target); el wrapper de FreeRTOS y los
// transportes reales llegan con F1-T08.

#include "mole.h"

#if MOLE_ENABLED

#include <atomic>
#include <cstring>

#include "mole_codec.h"
#include "mole_internal.h"
#include "mole_port.h"

namespace mole {
namespace detail {

namespace {

bool is_urgent(uint8_t type) {
    return type == REC_CHECK_FAIL || type == REC_PAUSED;
}

void open_frame(TaskState& st, uint64_t t_base) {
    // t_base = timestamp del primer record: garantiza dt_us >= 0 aunque el
    // record haya esperado en el ring (PR-10: el timestamp es de captura).
    st.frame_t_base = t_base;
    st.frame_opened_at = port::now_us();
    st.frame_len = kFrameHeaderLen;
    st.rec_count = 0;
    st.frame_open = true;
    st.urgent = false;
}

void emit_frame(TaskState& st, FrameSink sink, void* ctx) {
    if (!st.frame_open || st.rec_count == 0) {
        st.frame_open = false;
        return;
    }
    uint8_t* b = st.frame_buf;
    b[0] = static_cast<uint8_t>(kProtocolVersion | (st.flags_pending << 4));
    put_u16(b + 1, st.seq);
    put_u64(b + 3, st.frame_t_base);
    put_u16(b + 11, st.rec_count);
    const uint32_t crc = crc32(b, st.frame_len);
    put_u32(b + st.frame_len, crc);
    st.frame_len += 4;

    const size_t n = cobs_encode(st.wire_buf, sizeof st.wire_buf - 1, b, st.frame_len);
    st.wire_buf[n] = 0x00;
    if (sink(ctx, st.wire_buf, n + 1)) {
        st.tx_bytes += static_cast<uint32_t>(n + 1);
    }
    st.seq++;
    st.flags_pending = 0;
    st.frame_open = false;
}

// Agrega un record al frame abierto (abriendo/cerrando según haga falta).
void add_record(TaskState& st, FrameSink sink, void* ctx, uint8_t type,
                uint64_t t_us, const uint8_t* payload, uint8_t len,
                uint8_t flag) {
    for (int attempt = 0; attempt < 2; attempt++) {
        if (!st.frame_open) open_frame(st, t_us);
        // PR-08: dt_us es u16; si no entra, frame nuevo
        const uint64_t dt = t_us - st.frame_t_base;
        const bool dt_ok = t_us >= st.frame_t_base && dt <= 0xFFFF;
        // PR-04: el CRC tiene que entrar después
        const bool size_ok =
            st.frame_len + kRecordHeaderLen + len + 4 <= MOLE_FRAME_MAX;
        if (!dt_ok || !size_ok) {
            emit_frame(st, sink, ctx);
            continue;
        }
        uint8_t* p = st.frame_buf + st.frame_len;
        p[0] = type;
        p[1] = len;
        put_u16(p + 2, static_cast<uint16_t>(dt));
        std::memcpy(p + 4, payload, len);
        st.frame_len += kRecordHeaderLen + len;
        st.rec_count++;
        st.flags_pending |= flag;
        if (is_urgent(type)) st.urgent = true;
        return;
    }
    // record imposible de ubicar (no debería pasar: len<=255 << FRAME_MAX)
}

void maybe_stats(TaskState& st, FrameSink sink, void* ctx, uint64_t now) {
    const RingStats rs = ring_stats();
    const bool dropped_changed = rs.dropped != st.last_dropped_seen;
    // PR-13: cada 500 ms y ante cada cambio de dropped
    if (!dropped_changed && st.last_stats_us != 0 &&
        now - st.last_stats_us < 500'000) {
        return;
    }
    st.last_stats_us = now;
    if (dropped_changed) st.flags_pending |= FLAG_DROPS;
    st.last_dropped_seen = rs.dropped;

    uint8_t payload[40];
    put_u32(payload, rs.enqueued);
    put_u32(payload + 4, rs.dropped);
    for (int i = 0; i < 8; i++) put_u16(payload + 8 + 2 * i, rs.dropped_by_kind[i]);
    put_u16(payload + 24, rs.ring_high_water);
    put_u16(payload + 26, sym_overflow_count());
    put_u32(payload + 28, st.tx_bytes);
    put_u32(payload + 32, 0);  // free_heap: lo completa el wrapper de IDF
    put_u32(payload + 36, 0);  // min_free_heap
    add_record(st, sink, ctx, REC_STATS, now, payload, 40, 0);
}

void maybe_counters(TaskState& st, FrameSink sink, void* ctx, uint64_t now) {
    // REC-23: delta cada 250 ms
    if (st.last_counter_us != 0 && now - st.last_counter_us < 250'000) return;
    st.last_counter_us = now;
    uint8_t payload[255];
    const uint8_t n = counters_collect(payload);
    if (n > 0) {
        add_record(st, sink, ctx, REC_COUNTER, now, payload, n, 0);
    }
}

}  // namespace

// ---------------------------------------------------------------------------
// Sesión y downlink (F1-T09)
// ---------------------------------------------------------------------------

namespace {

void put_str_pr20(uint8_t* p, uint8_t& off, const char* s, size_t cap = 40) {
    size_t n = s ? std::strlen(s) : 0;
    if (n > cap) n = cap;
    p[off++] = static_cast<uint8_t>(n);
    std::memcpy(p + off, s, n);
    off = static_cast<uint8_t>(off + n);
}

void emit_session(TaskState& st, FrameSink sink, void* ctx) {
    port::SessionFields f{};
    port::session_fields(&f);
    uint8_t payload[255];
    uint8_t p = 0;
    put_u32(payload, f.epoch);
    put_u32(payload + 4, catalog_hash());
    put_u16(payload + 8, f.chip_model);
    put_u16(payload + 10, f.chip_rev);
    p = 12;
    put_str_pr20(payload, p, f.idf_ver);
    put_str_pr20(payload, p, f.app_name);
    put_str_pr20(payload, p, f.app_build_time);
    std::memcpy(payload + p, f.elf_sha, 8);
    p = static_cast<uint8_t>(p + 8);
    put_u16(payload + p, f.cpu_freq_mhz);
    p = static_cast<uint8_t>(p + 2);
    put_u32(payload + p, f.free_heap);
    p = static_cast<uint8_t>(p + 4);
    put_str_pr20(payload, p, "2.0.0-dev");
    add_record(st, sink, ctx, REC_SESSION, port::now_us(), payload, p, 0);
    st.last_session_us = port::now_us();
}

// CAT-10: re-emite el catálogo completo, en orden defs-antes-de-uso:
// símbolos → tipos/enums (en orden de registro: anidados primero) →
// formatos. NADA de esto toca el catalog_hash (el host lo recibe por
// REC_SESSION, no lo recomputa del stream).
void resync_catalog(TaskState& st, FrameSink sink, void* ctx) {
    uint8_t payload[255];
    uint8_t len = 0;
    const uint16_t nsyms = sym_table_count();
    for (uint16_t i = 0; i < nsyms; i++) {
        if (sym_table_entry(i, payload, &len)) {
            add_record(st, sink, ctx, REC_SYM_DEF, port::now_us(), payload, len, 0x1);
        }
    }
    const size_t ntypes = type_reemit_count();
    for (size_t i = 0; i < ntypes; i++) {
        run_type_reemit(i);  // encolan en la cola meta, sin hash
    }
    MetaView mv;
    uint8_t buf[255];
    while (meta_pop(&mv, buf)) {
        add_record(st, sink, ctx, mv.type, mv.t_us, mv.payload, mv.len, 0x1);
    }
    const uint16_t nfmts = fmt_table_count();
    for (uint16_t i = 0; i < nfmts; i++) {
        if (fmt_table_entry(i, payload, &len)) {
            add_record(st, sink, ctx, REC_FMT_DEF, port::now_us(), payload, len, 0x1);
        }
    }
}

void apply_ctl(TaskState& st, FrameSink sink, void* ctx, uint8_t type,
               const uint8_t* p, uint8_t len) {
    // SEC-03: antes de un handshake válido solo se acepta CTL_HELLO
    if (!st.hello_seen && type != CTL_HELLO) return;
    switch (type) {
        case CTL_HELLO: {
            if (len < 9) return;
            st.hello_seen = true;
            const uint32_t known_epoch = get_u32(p + 1);
            const uint32_t known_hash = get_u32(p + 5);
            port::SessionFields f{};
            port::session_fields(&f);
            if (known_epoch != f.epoch || known_hash != catalog_hash()) {
                resync_catalog(st, sink, ctx);  // CAT-10
            }
            emit_session(st, sink, ctx);
            break;
        }
        case CTL_PING: {
            if (len < 4) return;
            uint8_t pong[4];
            std::memcpy(pong, p, 4);
            add_record(st, sink, ctx, REC_PONG, port::now_us(), pong, 4, 0);
            break;
        }
        case CTL_SET_LEVEL: {
            if (len < 3) return;
            const uint16_t sym = get_u16(p);
            const auto lvl = static_cast<Level>(p[2]);
            if (sym == 0) {
                set_min_level(lvl);
            } else {
                set_tag_level(sym, lvl);
            }
            break;
        }
        case CTL_SET_POLICY: {
            if (len < 2 || p[0] > 7 || p[1] > 3) return;
            set_policy(static_cast<Channel>(p[0]), static_cast<Policy>(p[1]));
            break;
        }
        case CTL_RESET: {
            // SEC-02: exige el magic; un byte de ruido no reinicia nada
            if (len == 4 && get_u32(p) == kResetMagic) {
                port::reset_device();
            }
            break;
        }
        default:
            // CTL_CMD/BIND_SET (F3), PAUSE/RESUME/STEP (F6): todavía no
            break;
    }
}

}  // namespace

void downlink_feed(TaskState& st, FrameSink sink, void* ctx,
                   const uint8_t* data, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (st.rx_len < sizeof st.rx_buf) {
            st.rx_buf[st.rx_len++] = data[i];
        } else {
            st.rx_len = 0;  // basura interminable: descartar y contar
            st.rx_bad_frames++;
        }
        if (data[i] != 0x00) continue;
        // bloque completo (el 0x00 está incluido al final)
        const size_t block_len = st.rx_len - 1;
        st.rx_len = 0;
        if (block_len == 0) continue;
        uint8_t pre[512];
        size_t pre_n = 0;
        if (cobs_decode(pre, sizeof pre, &pre_n, st.rx_buf, block_len) != Err::Ok) {
            st.rx_bad_frames++;  // PR-16: descarte silencioso, contado
            continue;
        }
        FrameHeader h;
        if (frame_parse_header(pre, pre_n, &h) != Err::Ok) {
            st.rx_bad_frames++;
            continue;
        }
        RecordView recs[32];
        size_t count = 0;
        if (frame_records(pre, pre_n, recs, 32, &count) != Err::Ok) {
            st.rx_bad_frames++;
            continue;
        }
        for (size_t r = 0; r < count && r < 32; r++) {
            apply_ctl(st, sink, ctx, recs[r].type, recs[r].payload, recs[r].len);
        }
    }
}

uint32_t task_pump(TaskState& st, FrameSink sink, void* ctx) {
    uint32_t moved = 0;
    const uint64_t now = port::now_us();
    uint8_t buf[255];

    // 1) definiciones de catálogo primero: viajan antes que su primer uso
    MetaView mv;
    while (meta_pop(&mv, buf)) {
        add_record(st, sink, ctx, mv.type, mv.t_us, mv.payload, mv.len,
                   0x1 /* FLAG_CATALOG en el nibble */);
        moved++;
    }

    // 2) periódicos
    maybe_counters(st, sink, ctx, now);
    maybe_stats(st, sink, ctx, now);
    // CAT-11: REC_SESSION no solicitado cada 2 s hasta recibir CTL_HELLO
    if (!st.hello_seen &&
        (st.last_session_us == 0 || now - st.last_session_us >= 2'000'000)) {
        emit_session(st, sink, ctx);
    }

    // 3) rings de productores, round-robin con cuota por pasada (FW-06)
    bool any = true;
    while (any) {
        any = false;
        for (uint8_t slot = 0; slot < MOLE_MAX_PRODUCERS; slot++) {
            uint8_t type, len;
            uint64_t t;
            if (ring_pop_slot(slot, &type, &t, buf, &len)) {
                add_record(st, sink, ctx, type, t, buf, len, 0);
                moved++;
                any = true;
            }
        }
        uint8_t type, len;
        uint64_t t;
        if (isr_pop(&type, &t, buf, &len)) {
            add_record(st, sink, ctx, type, t, buf, len, 0);
            moved++;
            any = true;
        }
    }

    // 4) cierre por urgencia o por tiempo (PR-05 b/c)
    if (st.frame_open &&
        (st.urgent ||
         port::now_us() - st.frame_opened_at >=
             static_cast<uint64_t>(MOLE_FLUSH_MS) * 1000)) {
        emit_frame(st, sink, ctx);
    }
    return moved;
}

void task_flush(TaskState& st, FrameSink sink, void* ctx) {
    task_pump(st, sink, ctx);
    emit_frame(st, sink, ctx);
}

}  // namespace detail

// ---------------------------------------------------------------------------
// Wrapper de FreeRTOS + transporte (solo target). F1-T08; la verificación
// con host real queda para el frente hardware.
// ---------------------------------------------------------------------------

#ifdef ESP_PLATFORM

}  // namespace mole

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "mole_transport.h"

namespace mole {

namespace {

struct TaskCtx {
    detail::TaskState st;
    IMoleTransport* transport = nullptr;
    TaskHandle_t handle = nullptr;
    std::atomic<bool> flush_req{false};
    std::atomic<bool> stop_req{false};
};

TaskCtx& task_ctx() {
    static TaskCtx ctx;
    return ctx;
}

bool transport_sink(void* ctx, const uint8_t* wire, size_t len) {
    auto* t = static_cast<IMoleTransport*>(ctx);
    return t->write(wire, len);
}

void mole_task_main(void*) {
    TaskCtx& ctx = task_ctx();
    detail::ensure_isr_ring();
    uint8_t rx[512];
    while (!ctx.stop_req.load(std::memory_order_relaxed)) {
        // TR-08: sin host conectado no se bombea; los productores descartan
        if (ctx.transport->connected()) {
            detail::task_pump(ctx.st, transport_sink, ctx.transport);
            const size_t n = ctx.transport->read(rx, sizeof rx);
            if (n > 0) {
                detail::downlink_feed(ctx.st, transport_sink, ctx.transport, rx, n);
            }
            if (ctx.flush_req.exchange(false, std::memory_order_relaxed)) {
                detail::task_flush(ctx.st, transport_sink, ctx.transport);
            }
        }
        vTaskDelay(1);  // con FreeRTOS a 1000 Hz: PR-05(b) con FLUSH_MS>=1
    }
    ctx.handle = nullptr;
    vTaskDelete(nullptr);
}

}  // namespace

void begin(const Config& cfg) {
    TaskCtx& ctx = task_ctx();
    if (ctx.handle != nullptr) return;
    IMoleTransport* t = nullptr;
    if (cfg.transport == Transport::UsbCdc || cfg.transport == Transport::Auto) {
        t = transport_cdc();
    }
    if (t == nullptr && cfg.transport != Transport::UsbCdc) {
        t = transport_uart(cfg.baud);
    }
    if (t == nullptr) return;
    ctx.transport = t;
    ctx.stop_req.store(false);
    const UBaseType_t prio =
        cfg.task_prio != 0 ? cfg.task_prio : tskIDLE_PRIORITY + 2;
    xTaskCreatePinnedToCore(mole_task_main, "moleTask", 4096, nullptr, prio,
                            &ctx.handle, cfg.task_core < 0 ? tskNO_AFFINITY
                                                           : cfg.task_core);
}

void end() {
    task_ctx().stop_req.store(true, std::memory_order_relaxed);
}

void flush() {
    task_ctx().flush_req.store(true, std::memory_order_relaxed);
}

}  // namespace mole

namespace mole {

#endif  // ESP_PLATFORM

}  // namespace mole

#endif  // MOLE_ENABLED
