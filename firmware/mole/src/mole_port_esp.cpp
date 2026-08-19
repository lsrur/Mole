// SPDX-License-Identifier: MIT
// mole_port_esp.cpp — los hooks de puerto sobre ESP-IDF, llamadas directas
// (PLAT-05: sin HAL). Solo compila en el target.

#ifdef ESP_PLATFORM

#include "mole_port.h"

#include <atomic>

#include "esp_app_desc.h"
#include "esp_attr.h"
#include "esp_chip_info.h"
#include "esp_cpu.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "mole_config.h"
#include "rom/ets_sys.h"

// Índice de TLS de FreeRTOS para el slot de productor (FW-04). El 0 suele
// usarlo pthread; exige CONFIG_FREERTOS_THREAD_LOCAL_STORAGE_POINTERS >= 2.
#ifndef MOLE_TLS_INDEX
#define MOLE_TLS_INDEX 1
#endif

static_assert(configNUM_THREAD_LOCAL_STORAGE_POINTERS > MOLE_TLS_INDEX,
              "MOLE: subir CONFIG_FREERTOS_THREAD_LOCAL_STORAGE_POINTERS");

namespace mole {
namespace port {

namespace {
std::atomic<uint8_t> g_next_slot{0};
}

uint64_t now_us() {
    return static_cast<uint64_t>(esp_timer_get_time());
}

uint8_t task_slot() {
    // el puntero guarda slot+1 (0 = sin asignar)
    void* v = pvTaskGetThreadLocalStoragePointer(nullptr, MOLE_TLS_INDEX);
    if (v == nullptr) {
        const uint8_t slot = g_next_slot.fetch_add(1, std::memory_order_relaxed);
        if (slot >= MOLE_MAX_PRODUCERS) return 0xFF;
        vTaskSetThreadLocalStoragePointer(
            nullptr, MOLE_TLS_INDEX,
            reinterpret_cast<void*>(static_cast<uintptr_t>(slot) + 1));
        return slot;
    }
    return static_cast<uint8_t>(reinterpret_cast<uintptr_t>(v) - 1);
}

bool in_isr() {
    return xPortInIsrContext() != 0;
}

uint8_t core_id() {
    return static_cast<uint8_t>(xPortGetCoreID());
}

void yield_short() {
    vTaskDelay(1);
}

// epoch en RTC memory: sobrevive resets de software (CAT-07)
static RTC_NOINIT_ATTR uint32_t s_epoch_counter;

void session_fields(SessionFields* out) {
    static bool inited = false;
    if (!inited) {
        inited = true;
        s_epoch_counter++;  // basura tras power-on también sirve: es único
    }
    out->epoch = s_epoch_counter;
    esp_chip_info_t ci;
    esp_chip_info(&ci);
    out->chip_model = static_cast<uint16_t>(ci.model);
    out->chip_rev = static_cast<uint16_t>(ci.revision);
    out->idf_ver = IDF_VER;
    const esp_app_desc_t* app = esp_app_get_description();
    out->app_name = app->project_name;
    out->app_build_time = app->time;
    // 8 bytes del sha256 del ELF (CAT-12)
    for (int i = 0; i < 8; i++) out->elf_sha[i] = app->app_elf_sha256[i];
    out->cpu_freq_mhz = static_cast<uint16_t>(ets_get_cpu_frequency());
    out->free_heap = static_cast<uint32_t>(esp_get_free_heap_size());
}

void reset_device() {
    esp_restart();
}

}  // namespace port
}  // namespace mole

#endif  // ESP_PLATFORM
