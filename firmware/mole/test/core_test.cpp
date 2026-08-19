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

// ---------------------------------------------------------------------------
// F1-T05: descriptores (TEST-09)
// ---------------------------------------------------------------------------

enum class Mode : uint8_t { RAW = 0, AVG = 1, MEDIAN = 2 };
MOLE_DESCRIBE_ENUM(Mode, RAW, AVG, MEDIAN);

struct Sens {
    uint8_t a;
    uint16_t b;
};
MOLE_DESCRIBE_BE(Sens, a, b);

struct Imu {
    float ax[3];
    Mode mode;
};
MOLE_DESCRIBE(Imu, ax, mode);

struct Inner {
    int16_t val;
};
MOLE_DESCRIBE(Inner, val);

struct Outer {
    Inner inner;
    float f;
};
MOLE_DESCRIBE(Outer, inner, f);

struct MetaDef {
    uint8_t type;
    std::vector<uint8_t> payload;
};

static std::vector<MetaDef> drain_meta() {
    std::vector<MetaDef> out;
    mole::detail::MetaView mv;
    uint8_t buf[255];
    while (mole::detail::meta_pop(&mv, buf)) {
        out.push_back({mv.type, std::vector<uint8_t>(buf, buf + mv.len)});
    }
    return out;
}

static void test_descriptores() {
    mole::detail::reset_for_tests();

    // ---- Sens: BE por campo, offsets/sizeof reales del compilador ----
    static_assert(offsetof(Sens, b) == 2, "layout esperado");
    static_assert(sizeof(Sens) == 4, "padding esperado");
    Sens s;
    const uint8_t mem[4] = {0x11, 0xEE, 0x22, 0x33};  // b guardado BE
    std::memcpy(&s, mem, 4);
    MOLE_INFO("s={}", s);

    auto defs = drain_meta();
    // sym a, sym b, TYPE_DEF, sym archivo, FMT_DEF
    CHECK(defs.size() == 5);
    CHECK(defs[2].type == mole::REC_TYPE_DEF);
    const auto& td = defs[2].payload;
    const uint16_t sens_tid = mole::get_u16(td.data());
    CHECK(td[2] == 4 && std::memcmp(td.data() + 3, "Sens", 4) == 0);
    CHECK(td[7] == 2);  // nfields
    // campo a: wire u8, flags BE, offset 0, size 1
    CHECK(td[10] == mole::WIRE_U8 && td[11] == mole::kFieldFlagBE);
    CHECK(mole::get_u16(td.data() + 12) == 0 && mole::get_u16(td.data() + 14) == 1);
    // campo b: wire u16, flags BE, offset 2, size 2
    CHECK(td[20] == mole::WIRE_U16 && td[21] == mole::kFieldFlagBE);
    CHECK(mole::get_u16(td.data() + 22) == 2 && mole::get_u16(td.data() + 24) == 2);
    CHECK(defs[4].type == mole::REC_FMT_DEF);
    // tag del arg: 0xF0 + type_id inline (REC-44)
    CHECK(defs[4].payload[7] == mole::WIRE_STRUCT);
    CHECK(mole::get_u16(defs[4].payload.data() + 8) == sens_tid);

    // record: modo valores sin padding, bytes tal como estan en memoria —
    // exactamente el vector args-struct-values
    uint8_t buf[255], type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(len == 7 + 3);
    CHECK(hex(buf + 7, 3) == "112233");

    // ---- Imu: arreglo (REC-54) + enum descripto (REC-53) ----
    Imu imu{{1.0f, 2.0f, 3.0f}, Mode::AVG};
    MOLE_INFO("imu={}", imu);
    defs = drain_meta();
    const MetaDef* imu_td = nullptr;
    const MetaDef* enum_td = nullptr;
    for (const auto& d : defs) {
        if (d.type == mole::REC_TYPE_DEF) imu_td = &d;
        if (d.type == mole::REC_ENUM_DEF) enum_td = &d;
    }
    CHECK(imu_td && enum_td);
    // el ENUM_DEF viaja ANTES que el TYPE_DEF que lo referencia
    CHECK(&defs.front() <= enum_td && enum_td < imu_td);
    const uint16_t mode_tid = mole::get_u16(enum_td->payload.data());
    // base u8, 3 entradas: RAW=0, AVG=1, MEDIAN=2
    const auto& ep = enum_td->payload;
    const size_t base_off = 3 + ep[2];
    CHECK(ep[base_off] == mole::WIRE_U8);
    CHECK(ep[base_off + 1] == 3);
    CHECK(ep[base_off + 2] == 0 && ep[base_off + 5] == 1 && ep[base_off + 8] == 2);
    // campos de Imu: ax f32 ARRAY size 12; mode u8 ENUM ref=mode_tid
    const auto& ip = imu_td->payload;
    const size_t f0 = 3 + ip[2] + 1;
    CHECK(ip[f0 + 2] == mole::WIRE_F32 && ip[f0 + 3] == mole::kFieldFlagArray);
    CHECK(mole::get_u16(ip.data() + f0 + 6) == 12);
    CHECK(ip[f0 + 12] == mole::WIRE_U8 && ip[f0 + 13] == mole::kFieldFlagEnum);
    CHECK(mole::get_u16(ip.data() + f0 + 18) == mode_tid);
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(len == 7 + 13);
    CHECK(hex(buf + 7, 13) == "0000803f000000400000404001");

    // ---- enum suelto como argumento: tag 0xF1 + type_id (REC-53) ----
    Mode m = Mode::MEDIAN;
    MOLE_INFO("modo={}", m);
    defs = drain_meta();
    CHECK(defs.size() == 1 && defs[0].type == mole::REC_FMT_DEF);
    CHECK(defs[0].payload[7] == 0xF1);
    CHECK(mole::get_u16(defs[0].payload.data() + 8) == mode_tid);
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(len == 8 && buf[7] == 2);

    // ---- anidado: empaquetado secuencial sin padding (Q-3) ----
    Outer o{{-5}, 1.5f};
    MOLE_INFO("o={}", o);
    defs = drain_meta();
    int tds = 0;
    for (const auto& d : defs) {
        if (d.type == mole::REC_TYPE_DEF) tds++;
    }
    CHECK(tds == 2);  // Inner antes que Outer
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(len == 7 + 6);
    CHECK(hex(buf + 7, 6) == "fbff0000c03f");

    // ---- MOLE_BYTES: hexdump inline degradado (REC-48) ----
    MOLE_INFO("crudo {}", MOLE_BYTES(s));
    defs = drain_meta();
    CHECK(defs.size() == 1 && defs[0].type == mole::REC_FMT_DEF);
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(buf[7] == 8);  // prefijo PR-20: 4 bytes → 8 chars hex
    CHECK(std::memcmp(buf + 8, "11ee2233", 8) == 0);
}

// ---------------------------------------------------------------------------
// F1-T06: watch / watchEvery / count / event
// ---------------------------------------------------------------------------

static void test_watch_bytes_y_rate_limit() {
    mole::detail::reset_for_tests();
    mole::watch("Voltage", 1.5f);
    uint8_t buf[255], type, len;
    uint64_t t;
    // primero drena el sym def de la cola meta... el watch va por el ring
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(type == mole::REC_WATCH);
    CHECK(len == 7);
    // type+value identicos al vector rec-watch (el sym depende del catalogo)
    CHECK(hex(buf + 2, 5) == "090000c03f");

    mole::watch("estado", "JOINED");
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(type == mole::REC_WATCH_STR);
    CHECK(buf[2] == 6 && std::memcmp(buf + 3, "JOINED", 6) == 0);

    // watchEvery: dentro de la ventana descarta EN EL PRODUCTOR y no cuenta
    // como perdida (es rate limit deliberado, REC-09)
    mole::testhooks::set_time_us(10'000'000);
    mole::watchEvery("hf", 1, 50);
    mole::watchEvery("hf", 2, 50);
    mole::watchEvery("hf", 3, 50);
    int emitted = 0;
    while (pop_ring0(&type, &t, buf, &len)) emitted++;
    CHECK(emitted == 1);
    CHECK(mole::detail::ring_stats().dropped == 0);
    mole::testhooks::advance_us(60'000);  // pasa la ventana de 50 ms
    mole::watchEvery("hf", 4, 50);
    CHECK(pop_ring0(&type, &t, buf, &len));
}

static void test_counters_agregados() {
    mole::detail::reset_for_tests();
    for (int i = 0; i < 100000; i++) mole::count("irq_rx");
    mole::count("tx_done", 7);
    uint8_t payload[255];
    const uint8_t n = mole::detail::counters_collect(payload);
    CHECK(n == 1 + 2 * 6);
    CHECK(payload[0] == 2);
    // deltas: 100000 y 7 (el orden es el de registro)
    CHECK(mole::get_u32(payload + 3) == 100000);
    CHECK(mole::get_u32(payload + 9) == 7);
    // segunda recoleccion: nada (los deltas se resetean)
    CHECK(mole::detail::counters_collect(payload) == 0);

    // concurrencia: 4 hilos x 50k incrementos, suma exacta
    std::vector<std::thread> ts;
    for (int i = 0; i < 4; i++) {
        ts.emplace_back([] {
            for (int k = 0; k < 50000; k++) mole::count("irq_rx");
        });
    }
    for (auto& th : ts) th.join();
    const uint8_t n2 = mole::detail::counters_collect(payload);
    CHECK(n2 != 0);
    CHECK(mole::get_u32(payload + 3) == 200000);
}

static void test_event() {
    mole::detail::reset_for_tests();
    mole::event("sync", 42);
    uint8_t buf[255], type, len;
    uint64_t t;
    CHECK(pop_ring0(&type, &t, buf, &len));
    CHECK(type == mole::REC_EVENT);
    CHECK(len == 6);
    CHECK(mole::get_u32(buf + 2) == 42);
}

// ---------------------------------------------------------------------------
// F1-T07: la bomba de la moleTask, end-to-end en host
// ---------------------------------------------------------------------------

struct WireCapture {
    std::vector<std::vector<uint8_t>> frames;
    std::vector<uint8_t> stream;
};

static bool capture_sink(void* ctx, const uint8_t* wire, size_t len) {
    auto* cap = static_cast<WireCapture*>(ctx);
    cap->frames.emplace_back(wire, wire + len);
    cap->stream.insert(cap->stream.end(), wire, wire + len);
    return true;
}

static void test_pump_end_to_end() {
    mole::detail::reset_for_tests();
    mole::testhooks::set_time_us(1'000'000);

    // 3 productores reales emitiendo mientras la bomba corre
    const uint32_t kPerThread = 20000;
    std::atomic<bool> done{false};
    std::vector<std::thread> producers;
    for (int p = 0; p < 3; p++) {
        producers.emplace_back([p] {
            for (uint32_t i = 0; i < kPerThread; i++) {
                switch (p) {
                    case 0:
                        MOLE_INFO("crudo={} v={:.2}", static_cast<int32_t>(i), 2.47f);
                        break;
                    case 1:
                        mole::watch("Voltage", 1.5f);
                        break;
                    default:
                        mole::count("irq_rx");
                        break;
                }
                if (i % 64 == 0) std::this_thread::yield();
            }
        });
    }

    WireCapture cap;
    mole::detail::TaskState st;
    std::thread pump([&] {
        while (!done.load()) {
            mole::testhooks::advance_us(1000);  // el reloj avanza 1 ms por vuelta
            mole::detail::task_pump(st, capture_sink, &cap);
            std::this_thread::yield();
        }
        mole::testhooks::advance_us(400'000);  // vencer counters pendientes
        mole::detail::task_pump(st, capture_sink, &cap);
        mole::detail::task_flush(st, capture_sink, &cap);
    });
    for (auto& t : producers) t.join();
    done.store(true);
    pump.join();

    // decodificar TODO el stream con el codec (el mismo validado contra los
    // vectores compartidos): cuenta por tipo, CRC y seq
    uint32_t logs = 0, watches = 0, counter_delta_sum = 0, stats_recs = 0;
    uint32_t defs = 0, others = 0;
    uint16_t expected_seq = 0;
    bool seq_ok = true;
    for (const auto& f : cap.frames) {
        CHECK(f.back() == 0x00);
        uint8_t pre[MOLE_FRAME_MAX];
        size_t pre_n = 0;
        CHECK(mole::cobs_decode(pre, sizeof pre, &pre_n, f.data(), f.size() - 1) ==
              mole::Err::Ok);
        mole::FrameHeader h;
        CHECK(mole::frame_parse_header(pre, pre_n, &h) == mole::Err::Ok);
        if (h.seq != expected_seq) seq_ok = false;
        expected_seq = static_cast<uint16_t>(h.seq + 1);
        std::vector<mole::RecordView> recs(2048);
        size_t count = 0;
        CHECK(mole::frame_records(pre, pre_n, recs.data(), recs.size(), &count) ==
              mole::Err::Ok);
        for (size_t i = 0; i < count; i++) {
            switch (recs[i].type) {
                case mole::REC_LOG_FMT:
                    logs++;
                    break;
                case mole::REC_WATCH:
                    watches++;
                    break;
                case mole::REC_COUNTER: {
                    const uint8_t n = recs[i].payload[0];
                    for (uint8_t k = 0; k < n; k++) {
                        counter_delta_sum +=
                            mole::get_u32(recs[i].payload + 1 + 6 * k + 2);
                    }
                    break;
                }
                case mole::REC_STATS:
                    stats_recs++;
                    break;
                case mole::REC_SYM_DEF:
                case mole::REC_FMT_DEF:
                    defs++;
                    break;
                default:
                    others++;
                    break;
            }
        }
    }
    CHECK(seq_ok);
    CHECK(others == 0);
    CHECK(defs >= 3);  // Voltage, irq_rx, archivo, fmt...
    CHECK(stats_recs >= 1);

    // TEST-04 en miniatura: emitido = recibido + descartado, por canal
    const auto s = mole::detail::ring_stats();
    CHECK(logs + s.dropped_by_kind[mole::detail::CH_LOG] == kPerThread);
    CHECK(watches + s.dropped_by_kind[mole::detail::CH_WATCH] == kPerThread);
    CHECK(counter_delta_sum == kPerThread);  // los counters no se descartan: se agregan

    // el stream queda para la validacion cruzada con molectl (F1-T11)
    std::FILE* f = std::fopen("e2e_stream.bin", "wb");
    if (f) {
        std::fwrite(cap.stream.data(), 1, cap.stream.size(), f);
        std::fclose(f);
    }
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
    test_descriptores();
    test_watch_bytes_y_rate_limit();
    test_counters_agregados();
    test_event();
    test_pump_end_to_end();
    if (g_fails == 0) {
        std::puts("core_test: ok");
        return 0;
    }
    std::printf("core_test: %d fallos\n", g_fails);
    return 1;
}
