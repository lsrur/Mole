// SPDX-License-Identifier: MIT
// mole_port.h — la única costura entre el core y el mundo (R-2 del plan F1).
// NO es un HAL (PLAT-05): son los hooks mínimos para poder compilar y testear
// el core en host. En el target los implementa mole_port_esp.cpp con llamadas
// directas a ESP-IDF; en los tests, mole_port_host.cpp con mocks.
#pragma once

#include <stdint.h>

namespace mole {
namespace port {

// Timestamp monotónico en µs (esp_timer_get_time() en el target).
uint64_t now_us();

// Slot del productor actual: 0..MOLE_MAX_PRODUCERS-1, asignado en el primer
// uso desde una tarea nueva (FW-04, TLS de FreeRTOS en el target).
// Devuelve 0xFF si no hay slots libres.
uint8_t task_slot();

// ¿Estamos en contexto de interrupción? (FW-05)
bool in_isr();

// Core actual (xPortGetCoreID en el target; 0 en host).
uint8_t core_id();

// Espera corta para la política Block (vTaskDelay en el target). El
// deadline se controla con now_us(); esto solo cede la CPU.
void yield_short();

}  // namespace port
}  // namespace mole
