// SPDX-License-Identifier: MIT
// Sonda de FW-12: compilada con MOLE_ENABLED=0, ninguno de estos literales
// puede sobrevivir en el binario. El test corre `strings` y exige cero
// apariciones de SECRETO.

#include "mole.h"

struct ProbePkt {
    float x;
    int n;
};
MOLE_DESCRIBE(ProbePkt, x, n);

int main() {
    ProbePkt p{2.47f, 7};
    MOLE_INFO("SECRETO_LOG={} {}", 1, p.x);
    MOLE_WARN_T(SECRETO_TAG, "SECRETO_FMT {}", p.n);
    mole::watch("SECRETO_WATCH", p.x);
    mole::watchEvery("SECRETO_EVERY", p.n, 50);
    mole::count("SECRETO_COUNT");
    mole::event("SECRETO_EVENT", 1);
    mole::logs(mole::Level::Info, "SECRETO_RUNTIME");
    return static_cast<int>(mole::stats().enqueued);
}
