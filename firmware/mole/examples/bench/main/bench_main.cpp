// SPDX-License-Identifier: MIT
// Firmware de banco (F1-T13). La emisión a tasa creciente llega con T-08/T-13;
// mientras tanto ejercita el camino de log con float para PERF-15: el build
// de control (BENCH_CONTROL_PRINTF) linkea el printf de floats de newlib y
// el normal no.

#include "mole.h"

#if defined(BENCH_CONTROL_PRINTF)
#include <cstdio>
#endif

extern "C" void app_main() {
    volatile float v = 2.47f;
    // ambos builds linkean Mole entero; el control AGREGA printf("%f") y el
    // delta de flash aísla el soporte de float de newlib (PERF-15)
    MOLE_INFO("v={:.2}", static_cast<float>(v));
    (void)mole::stats();
#if defined(BENCH_CONTROL_PRINTF)
    std::printf("v=%f\n", static_cast<double>(v));
#endif
}
