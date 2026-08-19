// SPDX-License-Identifier: MIT
// Firmware de banco (F1-T13). Por ahora: esqueleto que compila y enlaza el
// componente; la emisión a tasa creciente llega con las tareas T-08/T-13.

#include "mole.h"

extern "C" void app_main() {
#if MOLE_ENABLED
    (void)mole::stats();
#endif
}
