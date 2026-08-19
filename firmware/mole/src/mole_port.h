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

// Identidad de sesión (CAT-07/CAT-09). Las cadenas son estáticas.
struct SessionFields {
    uint32_t epoch;
    uint16_t chip_model;
    uint16_t chip_rev;
    const char* idf_ver;
    const char* app_name;
    const char* app_build_time;
    uint8_t elf_sha[8];
    uint16_t cpu_freq_mhz;
    uint32_t free_heap;
};
void session_fields(SessionFields* out);

// Reinicio del dispositivo (CTL_RESET con magic, SEC-02). En host: contador.
void reset_device();

}  // namespace port
}  // namespace mole
