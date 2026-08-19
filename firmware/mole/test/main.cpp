// SPDX-License-Identifier: MIT
// Runner de tests del codec C++. Consume protocol/codec_vectors.json (T-14).

#include "mole_codec.h"

#include <cstdio>

int main() {
    static_assert(mole::kProtocolVersion == 2, "version de protocolo");
    std::puts("codec_test: esqueleto ok (runner real en T-14)");
    return 0;
}
