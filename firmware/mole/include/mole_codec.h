// SPDX-License-Identifier: MIT
// mole_codec.h — COBS + CRC32 + armado de frames y records.
// Compila en host (g++/CMake) y en ESP32. Sin Arduino.h, sin ESP-IDF,
// sin asignación dinámica (C-4). Se implementa en T-13.
#pragma once

#include "mole_wire.h"
