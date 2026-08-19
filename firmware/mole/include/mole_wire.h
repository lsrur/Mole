// SPDX-License-Identifier: MIT
// mole_wire.h — opcodes, layouts y constantes del protocolo Mole v2.
// Solo declaraciones, sin lógica (R-3). Autoridad: specs/mole-spec.md §6/§7.
#pragma once

#include <stdint.h>

namespace mole {

// PR-02: bits 0-3 de ver_flags
inline constexpr uint8_t kProtocolVersion = 2;

}  // namespace mole
