// SPDX-License-Identifier: MIT
// Tests de host del core (F1-T02+). Se compila con -DMOLE_MAX_SYMBOLS=4
// para poder probar el overflow sin registrar 512 símbolos.

#include <cstdio>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

#include "mole.h"
#include "mole_codec.h"
#include "mole_internal.h"
#include "mole_port.h"

static int g_fails = 0;

#define CHECK(cond)                                                     \
    do {                                                                \
        if (!(cond)) {                                                  \
            std::printf("  FALLO %s:%d: %s\n", __FILE__, __LINE__, #cond); \
            g_fails++;                                                  \
        }                                                               \
    } while (0)

static std::string hex(const uint8_t* p, size_t n) {
    std::string s;
    char tmp[3];
    for (size_t i = 0; i < n; i++) {
        std::snprintf(tmp, sizeof tmp, "%02x", p[i]);
        s += tmp;
    }
    return s;
}

// ---------------------------------------------------------------------------
// F1-T02: intern y tabla de símbolos
// ---------------------------------------------------------------------------

static void test_intern_secuencial_y_dedup() {
    mole::detail::reset_for_tests();
    const mole::SymId a = mole::intern("net", mole::KIND_TAG);
    const mole::SymId b = mole::intern("imu", mole::KIND_TAG);
    CHECK(a == 1);  // CAT-01: secuencial desde 1; 0 es "sin símbolo" (CAT-06)
    CHECK(b == 2);
    // dedup por puntero y por contenido
    CHECK(mole::intern("net", mole::KIND_TAG) == a);
    const char dyn[] = {'n', 'e', 't', '\0'};
    CHECK(mole::intern(dyn, mole::KIND_TAG) == a);
    // mismo nombre con otro kind es OTRO símbolo
    CHECK(mole::intern("net", mole::KIND_TASK) == 3);
    // parent participa de la identidad (CAT-04)
    CHECK(mole::intern("net", mole::KIND_TAG, 2) == 4);
}

static void test_sym_def_bytes_contra_vector() {
    mole::detail::reset_for_tests();
    // el vector rec-sym-def: sym 1, kind Tag(2), parent 0, "net"
    CHECK(mole::intern("net", mole::KIND_TAG) == 1);
    mole::detail::MetaView mv;
    uint8_t buf[255];
    CHECK(mole::detail::meta_pop(&mv, buf));
    CHECK(mv.type == mole::REC_SYM_DEF);
    CHECK(hex(mv.payload, mv.len) == "0100020000036e6574");
    // catalog_hash == CRC32 del payload (CAT-08)
    const uint8_t payload[] = {0x01, 0x00, 0x02, 0x00, 0x00, 0x03, 'n', 'e', 't'};
    CHECK(mole::detail::catalog_hash() == mole::crc32(payload, sizeof payload));
    CHECK(!mole::detail::meta_pop(&mv, buf));  // una sola definición
}

static void test_overflow_contabilizado_sin_reintento() {
    mole::detail::reset_for_tests();
    CHECK(MOLE_MAX_SYMBOLS == 4);  // este binario se compila con el límite en 4
    // intern() guarda el puntero: los nombres DEBEN ser literales o memoria
    // estable (CAT-02). Un buffer reutilizado es un error de uso.
    const char* names[4] = {"s0", "s1", "s2", "s3"};
    for (int i = 0; i < 4; i++) {
        CHECK(mole::intern(names[i], mole::KIND_WATCH) == i + 1);
    }
    CHECK(mole::intern("desborde", mole::KIND_WATCH) == mole::kSymOverflow);
    CHECK(mole::detail::sym_overflow_count() == 1);
    // reintento del MISMO nombre: cuenta de nuevo pero no emite nada (CAT-05)
    CHECK(mole::intern("desborde", mole::KIND_WATCH) == mole::kSymOverflow);
    CHECK(mole::detail::sym_overflow_count() == 2);
    // los ya registrados siguen resolviendo
    CHECK(mole::intern("s0", mole::KIND_WATCH) == 1);
    // en la cola hay exactamente 4 definiciones, ninguna del desborde
    mole::detail::MetaView mv;
    uint8_t buf[255];
    int defs = 0;
    while (mole::detail::meta_pop(&mv, buf)) defs++;
    CHECK(defs == 4);
}

static void test_intern_concurrente() {
    mole::detail::reset_for_tests();
    // 4 hilos internando los mismos 3 nombres: ids consistentes, sin dobles
    std::vector<std::thread> ts;
    mole::SymId ids[4][3];
    const char* names[3] = {"a", "b", "c"};
    for (int t = 0; t < 4; t++) {
        ts.emplace_back([t, &ids, &names] {
            for (int i = 0; i < 3; i++) {
                ids[t][i] = mole::intern(names[i], mole::KIND_TAG);
            }
        });
    }
    for (auto& th : ts) th.join();
    for (int t = 1; t < 4; t++) {
        for (int i = 0; i < 3; i++) {
            CHECK(ids[t][i] == ids[0][i]);
        }
    }
    mole::detail::MetaView mv;
    uint8_t buf[255];
    int defs = 0;
    while (mole::detail::meta_pop(&mv, buf)) defs++;
    CHECK(defs == 3);  // una definición por símbolo, no por hilo
}

// ---------------------------------------------------------------------------
// F1-T03: rings SPSC y políticas
// ---------------------------------------------------------------------------

namespace mole {
namespace testhooks {
void set_time_us(uint64_t t);
void advance_us(uint64_t dt);
}  // namespace testhooks
}  // namespace mole

static void test_ring_roundtrip_y_wrap() {
    mole::detail::reset_for_tests();
    // muchas vueltas al ring con payloads de tamaño variable: el wrap se
    // ejercita seguro (4096 no es múltiplo de los tamaños usados)
    uint8_t payload[64];
    uint8_t buf[255];
    uint32_t emitidos = 0, recibidos = 0;
    for (int round = 0; round < 2000; round++) {
        const uint8_t len = static_cast<uint8_t>(7 + (round * 13) % 50);
        for (uint8_t i = 0; i < len; i++) payload[i] = static_cast<uint8_t>(round + i);
        if (mole::detail::ring_push(mole::REC_WATCH, payload, len)) emitidos++;
        // drenar cada 3 pushes para forzar posiciones de wrap distintas
        if (round % 3 == 0) {
            uint8_t type, rlen;
            uint64_t t;
            while (mole::detail::ring_pop_slot(mole::port::task_slot(), &type, &t, buf, &rlen)) {
                recibidos++;
                CHECK(type == mole::REC_WATCH);
            }
        }
    }
    uint8_t type, rlen;
    uint64_t t;
    while (mole::detail::ring_pop_slot(mole::port::task_slot(), &type, &t, buf, &rlen)) recibidos++;
    CHECK(emitidos == recibidos);
    CHECK(mole::detail::ring_stats().dropped == 0);
}

static void test_drop_newest_contabilizado() {
    mole::detail::reset_for_tests();
    uint8_t payload[200] = {0};
    // llenar el ring de 4096: cada entrada pesa 12+200; entran 19
    uint32_t ok = 0, drops = 0;
    for (int i = 0; i < 40; i++) {
        if (mole::detail::ring_push(mole::REC_LOG_FMT, payload, 200)) {
            ok++;
        } else {
            drops++;
        }
    }
    CHECK(ok == 19);
    CHECK(drops == 21);
    const auto s = mole::detail::ring_stats();
    CHECK(s.dropped == 21);
    CHECK(s.dropped_by_kind[mole::detail::CH_LOG] == 21);
    CHECK(s.dropped_by_kind[mole::detail::CH_WATCH] == 0);
    CHECK(s.enqueued == 19);
    CHECK(s.ring_high_water > 4000);
}

static void test_block_con_timeout() {
    mole::detail::reset_for_tests();
    mole::detail::set_policy(mole::detail::CH_LOG, mole::detail::Policy::Block);
    mole::testhooks::set_time_us(1000);
    uint8_t payload[200] = {0};
    for (int i = 0; i < 19; i++) {
        CHECK(mole::detail::ring_push(mole::REC_LOG_FMT, payload, 200));
    }
    // ring lleno y nadie drena: otro hilo avanza el reloj más allá del
    // deadline de FW-08 y el push debe volver con descarte contabilizado
    std::thread clock_thread([] {
        for (int i = 0; i < 200; i++) {
            mole::testhooks::advance_us(200);  // 40 ms en total
            std::this_thread::yield();
        }
    });
    const bool pushed = mole::detail::ring_push(mole::REC_LOG_FMT, payload, 200);
    clock_thread.join();
    CHECK(!pushed);
    CHECK(mole::detail::ring_stats().dropped == 1);

    // con un consumidor drenando, Block espera y NO pierde
    mole::detail::reset_for_tests();
    mole::detail::set_policy(mole::detail::CH_LOG, mole::detail::Policy::Block);
    mole::testhooks::set_time_us(1000);
    for (int i = 0; i < 19; i++) {
        CHECK(mole::detail::ring_push(mole::REC_LOG_FMT, payload, 200));
    }
    std::thread consumer([] {
        uint8_t buf[255], type, len;
        uint64_t t;
        // dar aire y drenar dos entradas
        std::this_thread::yield();
        mole::detail::ring_pop_slot(0, &type, &t, buf, &len);
        mole::detail::ring_pop_slot(0, &type, &t, buf, &len);
        mole::testhooks::advance_us(500);
    });
    const bool pushed2 = mole::detail::ring_push(mole::REC_LOG_FMT, payload, 200);
    consumer.join();
    CHECK(pushed2);
    CHECK(mole::detail::ring_stats().dropped == 0);
}

static void test_decimate_bajo_presion() {
    mole::detail::reset_for_tests();
    mole::detail::set_policy(mole::detail::CH_WATCH, mole::detail::Policy::Decimate);
    uint8_t payload[100] = {0};
    // sin presión (ring < 3/4) no descarta nada
    for (int i = 0; i < 10; i++) {
        CHECK(mole::detail::ring_push(mole::REC_WATCH, payload, 100));
    }
    CHECK(mole::detail::ring_stats().dropped == 0);
    // llevar el ring por encima de 3/4 (4096*3/4 = 3072; cada entrada 112)
    for (int i = 0; i < 18; i++) {
        mole::detail::ring_push(mole::REC_WATCH, payload, 100);
    }
    const auto antes = mole::detail::ring_stats();
    CHECK(antes.ring_high_water >= 3072);
    // ahora bajo presión: de 8 intentos, varios deben decimarse aunque
    // quede lugar físico
    uint32_t decimados = 0;
    for (int i = 0; i < 8; i++) {
        if (!mole::detail::ring_push(mole::REC_WATCH, payload, 100)) decimados++;
    }
    CHECK(decimados >= 3);
    CHECK(mole::detail::ring_stats().dropped >= decimados);
}

static void test_isr_ring() {
    mole::detail::reset_for_tests();
    mole::detail::ensure_isr_ring();
    const uint8_t ev[] = {0x12, 0x00, 0x2a, 0x00, 0x00, 0x00};  // sym 18, arg 42
    CHECK(mole::detail::isr_push(mole::REC_EVENT, ev, sizeof ev));
    uint8_t buf[255], type, len;
    uint64_t t;
    CHECK(mole::detail::isr_pop(&type, &t, buf, &len));
    CHECK(type == mole::REC_EVENT);
    CHECK(len == sizeof ev);
    CHECK(std::memcmp(buf, ev, sizeof ev) == 0);
    CHECK(!mole::detail::isr_pop(&type, &t, buf, &len));
}

static void test_spsc_concurrente() {
    mole::detail::reset_for_tests();
    // productor y consumidor reales: total emitido = recibido + descartado,
    // y los payloads llegan íntegros y en orden
    std::atomic<bool> done{false};
    std::atomic<uint32_t> received{0};
    uint32_t bad = 0;
    std::thread consumer([&] {
        uint8_t buf[255], type, len;
        uint64_t t;
        uint32_t expected_seq = 0;
        while (!done.load() || true) {
            if (mole::detail::ring_pop_slot(0, &type, &t, buf, &len)) {
                uint32_t seq;
                std::memcpy(&seq, buf, 4);
                // en orden estricto (SPSC): las secuencias solo crecen
                if (seq < expected_seq) bad++;
                expected_seq = seq + 1;
                received.fetch_add(1);
            } else if (done.load()) {
                break;
            }
        }
    });
    const uint32_t kTotal = 200000;
    uint32_t sent = 0;
    for (uint32_t i = 0; i < kTotal; i++) {
        uint8_t payload[16];
        std::memcpy(payload, &i, 4);
        if (mole::detail::ring_push(mole::REC_WATCH, payload, 16)) sent++;
    }
    done.store(true);
    consumer.join();
    CHECK(bad == 0);
    CHECK(sent == received.load());
    const auto s = mole::detail::ring_stats();
    CHECK(s.enqueued == sent);
    CHECK(s.enqueued + s.dropped == kTotal);
}

// ---------------------------------------------------------------------------
// F1-T04: macros de log diferido
// ---------------------------------------------------------------------------

static bool pop_meta(mole::detail::MetaView* mv, uint8_t* buf) {
    return mole::detail::meta_pop(mv, buf);
}

static bool pop_ring0(uint8_t* type, uint64_t* t, uint8_t* buf, uint8_t* len) {
    return mole::detail::ring_pop_slot(0, type, t, buf, len);
}

static void test_log_fmt_def_y_record() {
    mole::detail::reset_for_tests();
    // el mismo SITIO ejecutado dos veces: la primera registra, la segunda no
    int32_t vals[2] = {1024, 7};
    float fvals[2] = {2.47f, 0.5f};
    const int line_antes = __LINE__;
    for (int i = 0; i < 2; i++) MOLE_INFO("crudo={} v={:.2}", vals[i], fvals[i]);

    // 1) la primera ejecución emitió REC_SYM_DEF (el archivo) y REC_FMT_DEF
    mole::detail::MetaView mv;
    uint8_t buf[255];
    CHECK(pop_meta(&mv, buf));
    CHECK(mv.type == mole::REC_SYM_DEF);  // __FILE__ como KIND_FILE
    CHECK(pop_meta(&mv, buf));
    CHECK(mv.type == mole::REC_FMT_DEF);
    // payload REC-34: fmt_id=1, file_sym=1, line, argc=2, tags i32+f32, fmt
    CHECK(mole::get_u16(mv.payload) == 1);
    CHECK(mole::get_u16(mv.payload + 2) == 1);
    CHECK(mole::get_u16(mv.payload + 4) == line_antes + 1);
    CHECK(mv.payload[6] == 2);
    CHECK(mv.payload[7] == mole::WIRE_I32);
    CHECK(mv.payload[8] == mole::WIRE_F32);
    CHECK(mv.payload[9] == 16);  // strlen("crudo={} v={:.2}")
    CHECK(std::memcmp(mv.payload + 10, "crudo={} v={:.2}", 16) == 0);
    CHECK(!pop_meta(&mv, buf));

    // 2) el record en el ring: header + args EXACTOS al vector rec-log-fmt
    uint8_t type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(type == mole::REC_LOG_FMT);
    CHECK(len == 15);
    CHECK(buf[0] == 2);  // INFO
    CHECK(mole::get_u16(buf + 3) == 0);  // sin tag
    CHECK(mole::get_u16(buf + 5) == 1);  // fmt_id
    CHECK(hex(buf + 7, 8) == "000400007b141e40");  // args del vector

    // 3) la segunda pasada del loop no re-registró nada (registro perezoso)
    CHECK(!pop_meta(&mv, buf));
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(mole::get_u16(buf + 5) == 1);  // mismo fmt_id
}

static void test_log_tag_y_filtro_en_productor() {
    mole::detail::reset_for_tests();
    MOLE_WARN_T(net, "reintento {}/{}", 1, 5);
    mole::detail::MetaView mv;
    uint8_t buf[255];
    // defs: sym "net" (Tag), sym __FILE__ (File), fmt
    int defs = 0;
    mole::SymId net_sym = 0;
    while (pop_meta(&mv, buf)) {
        if (mv.type == mole::REC_SYM_DEF && buf[2] == mole::KIND_TAG) {
            net_sym = mole::get_u16(buf);
            CHECK(std::memcmp(buf + 6, "net", 3) == 0);
        }
        defs++;
    }
    CHECK(defs == 3);
    CHECK(net_sym != 0);
    uint8_t type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(mole::get_u16(buf + 3) == net_sym);

    // silenciar el tag (PR-19): el productor no encola NADA
    mole::set_tag_level(net_sym, mole::Level::Error);
    MOLE_WARN_T(net, "reintento {}/{}", 2, 5);
    CHECK(!pop_ring0(&type, &t, buf, &len));
    CHECK(mole::detail::ring_stats().enqueued == 1);  // solo el primero
    // elevar de nuevo
    mole::set_tag_level(net_sym, mole::Level::Trace);
    MOLE_WARN_T(net, "reintento {}/{}", 3, 5);
    CHECK(pop_ring0(&type, &t, buf, &len));
}

static void test_cadenas_literal_vs_runtime() {
    mole::detail::reset_for_tests();
    const char* runtime = "JOINED";
    MOLE_INFO("estado {} extra={}", "fijo", runtime);
    mole::detail::MetaView mv;
    uint8_t buf[255];
    uint8_t tags[2] = {0, 0};
    while (pop_meta(&mv, buf)) {
        if (mv.type == mole::REC_FMT_DEF) {
            tags[0] = mv.payload[7];
            tags[1] = mv.payload[8];
        }
    }
    CHECK(tags[0] == mole::WIRE_SYM);  // literal internada (REC-38)
    CHECK(tags[1] == mole::WIRE_STR);  // runtime inline
    uint8_t type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    // payload: hdr(7) + sym(2) + str(1+6)
    if (len != 16) std::printf("  len=%d payload=%s\n", len, hex(buf, len).c_str());
    CHECK(len == 7 + 2 + 1 + 6);
    CHECK(buf[9] == 6);
    CHECK(std::memcmp(buf + 10, "JOINED", 6) == 0);
}

static void test_logs_fallback() {
    mole::detail::reset_for_tests();
    mole::logs(mole::Level::Error, "armado en runtime");
    uint8_t buf[255], type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(type == mole::REC_LOG);
    CHECK(buf[0] == 4);
    CHECK(buf[9] == 17);
    CHECK(std::memcmp(buf + 10, "armado en runtime", 17) == 0);
}

int main() {
    test_intern_secuencial_y_dedup();
    test_sym_def_bytes_contra_vector();
    test_overflow_contabilizado_sin_reintento();
    test_intern_concurrente();
    test_ring_roundtrip_y_wrap();
    test_drop_newest_contabilizado();
    test_block_con_timeout();
    test_decimate_bajo_presion();
    test_isr_ring();
    test_spsc_concurrente();
    test_log_fmt_def_y_record();
    test_log_tag_y_filtro_en_productor();
    test_cadenas_literal_vs_runtime();
    test_logs_fallback();
    if (g_fails == 0) {
        std::puts("core_test: ok");
        return 0;
    }
    std::printf("core_test: %d fallos\n", g_fails);
    return 1;
}
