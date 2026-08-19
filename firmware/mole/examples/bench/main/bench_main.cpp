// SPDX-License-Identifier: MIT
// Firmware de banco de F1 (T-13). Produce las mediciones de §4 del plan:
//
//   PERF-01/02/14  costo por llamada, por contador de ciclos (M-4)
//   PERF-12        RAM antes/después de begin() (M-5)
//   PERF-04/05/06  rampa de emisión sostenida por canal (M-1/M-2)
//   PERF-07        ráfaga (M-3)
//   TEST-04        attempted se publica por el propio enlace: el host
//                  verifica attempted == recibidos + drops reportados
//
// Todo el reporte viaja como logs Mole: `molectl bench` lo consume.
// El build de control para PERF-15 sigue activo via BENCH_CONTROL_PRINTF.

#include <algorithm>
#include <cstdio>

#include "esp_cpu.h"
#include "esp_heap_caps.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "mole.h"

// struct representativo para PERF-14 (6 campos, como pide la spec)
struct SensorSample {
    float ax, ay, az;
    uint16_t t_ms;
    uint8_t mode;
    bool valid;
};
MOLE_DESCRIBE(SensorSample, ax, ay, az, t_ms, mode, valid);

namespace {

uint32_t free_heap_now() {
    return static_cast<uint32_t>(heap_caps_get_free_size(MALLOC_CAP_8BIT));
}

// p50/p99 de un lote de muestras de ciclos (M-4). 2000 muestras, en
// régimen: la primera llamada (registro perezoso) se descarta.
template <class F>
void measure_cost(const char* name, F&& op) {
    static uint32_t samples[2000];
    op();  // primera ejecución: registra defs, fuera de la medición
    for (auto& s : samples) {
        const uint32_t c0 = esp_cpu_get_cycle_count();
        op();
        s = esp_cpu_get_cycle_count() - c0;
    }
    std::sort(samples, samples + 2000);
    // a 240 MHz: 240 ciclos = 1 µs
    MOLE_INFO_T(bench, "cost {} p50={} p99={} ciclos", name, samples[1000],
                samples[1980]);
    mole::flush();
    vTaskDelay(pdMS_TO_TICKS(50));
}

void measure_costs() {
    volatile int32_t vi = 1024;
    volatile float vf = 2.47f;
    SensorSample s{0.12f, -9.79f, 0.03f, 1204, 1, true};
    measure_cost("log2esc", [&] { MOLE_INFO("adc={} v={:.2}", (int32_t)vi, (float)vf); });
    measure_cost("watch", [&] { mole::watch("bench.v", (float)vf); });
    measure_cost("struct6", [&] { MOLE_INFO("s={}", s); });
    measure_cost("count", [&] { mole::count("bench.c"); });
}

// Un paso de la rampa: emite a `rate` rec/s durante `secs`, mitad logs y
// mitad watches, y publica attempted al cerrar (TEST-04).
void ramp_step(uint32_t rate, uint32_t secs) {
    const uint32_t per_tick = rate / 100;  // tick de 10 ms
    uint32_t attempted_logs = 0, attempted_watches = 0;
    MOLE_INFO_T(bench, "step start rate={}", rate);
    mole::flush();
    const uint64_t t_end =
        static_cast<uint64_t>(secs) * 100;  // ticks de 10 ms
    for (uint64_t tick = 0; tick < t_end; tick++) {
        for (uint32_t i = 0; i < per_tick; i++) {
            if (i & 1) {
                mole::watch("bench.sig", static_cast<float>(i));
                attempted_watches++;
            } else {
                MOLE_INFO("crudo={} v={:.2}", static_cast<int32_t>(i), 2.47f);
                attempted_logs++;
            }
        }
        vTaskDelay(pdMS_TO_TICKS(10));
    }
    const mole::Stats st = mole::stats();
    MOLE_INFO_T(bench,
                "step done rate={} logs={} watches={} enq={} drop={} heap={}",
                rate, attempted_logs, attempted_watches, st.enqueued,
                st.dropped, free_heap_now());
    mole::flush();
    vTaskDelay(pdMS_TO_TICKS(500));  // drenar antes del próximo paso
}

// PERF-07: ráfaga a máxima velocidad de producción, sin pacing.
void burst_step(uint32_t n) {
    MOLE_INFO_T(bench, "burst start n={}", n);
    mole::flush();
    vTaskDelay(pdMS_TO_TICKS(200));
    const mole::Stats antes = mole::stats();
    for (uint32_t i = 0; i < n; i++) {
        mole::watch("bench.burst", i);
    }
    vTaskDelay(pdMS_TO_TICKS(1000));  // drenar
    const mole::Stats despues = mole::stats();
    MOLE_INFO_T(bench, "burst done n={} enq={} drop={}", n,
                despues.enqueued - antes.enqueued,
                despues.dropped - antes.dropped);
    mole::flush();
}

void bench_task(void*) {
    // PERF-12: el heap ya fue muestreado antes y después de begin()
    measure_costs();

    // M-1/M-2: rampa sostenida. El último escalón que cierra con drop=0
    // es el PERF-06 medido.
    static const uint32_t kRates[] = {1000,  2000,  5000,  10000,
                                      20000, 50000, 80000, 120000};
    for (uint32_t rate : kRates) {
        ramp_step(rate, 5);
    }
    burst_step(8000);
    MOLE_INFO_T(bench, "bench fin");
    mole::flush();
    for (;;) {
        // queda vivo emitiendo suave, útil para molectl watch
        mole::watch("bench.idle", free_heap_now());
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

}  // namespace

extern "C" void app_main() {
    volatile float v = 2.47f;
    // PERF-15: el control agrega printf("%f") y fuerza newlib full
    MOLE_INFO("v={:.2}", static_cast<float>(v));
#if defined(BENCH_CONTROL_PRINTF)
    std::printf("v=%f\n", static_cast<double>(v));
#endif

    const uint32_t heap_antes = free_heap_now();
    mole::begin();
    const uint32_t heap_despues = free_heap_now();
    // se emite apenas alguien conecte (queda en el ring hasta entonces)
    MOLE_INFO_T(bench, "heap antes={} despues={} delta={}", heap_antes,
                heap_despues, heap_antes - heap_despues);

    xTaskCreatePinnedToCore(bench_task, "bench", 8192, nullptr,
                            tskIDLE_PRIORITY + 1, nullptr, 1);
}
