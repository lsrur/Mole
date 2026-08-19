// SPDX-License-Identifier: MIT
// mole_task.cpp — la bomba de la moleTask (F1-T07): drenaje round-robin
// (FW-06), armado de frames con cierre por tamaño/tiempo/urgencia (PR-05,
// PR-08), REC_STATS (PR-13) y counters batcheados (REC-23).
//
// Esta parte es portable (host y target); el wrapper de FreeRTOS y los
// transportes reales llegan con F1-T08.

#include "mole.h"

#if MOLE_ENABLED

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
}  // namespace mole

#endif  // MOLE_ENABLED
