// SPDX-License-Identifier: MIT
// Mocks de los hooks de puerto para los tests de host (R-2 del plan F1).

#include "mole_port.h"

#include <atomic>
#include <thread>

namespace mole {
namespace port {

namespace {
std::atomic<uint64_t> g_fake_us{1000};
std::atomic<uint8_t> g_next_slot{0};
thread_local uint8_t t_slot = 0xFF;
}  // namespace

uint64_t now_us() {
    return g_fake_us.load(std::memory_order_relaxed);
}

uint8_t task_slot() {
    if (t_slot == 0xFF) {
        t_slot = g_next_slot.fetch_add(1, std::memory_order_relaxed);
    }
    return t_slot;
}

bool in_isr() {
    return false;
}

uint8_t core_id() {
    return 0;
}

void yield_short() {
    std::this_thread::yield();
}

namespace {
std::atomic<uint32_t> g_resets{0};
}

void session_fields(SessionFields* out) {
    out->epoch = 0x11223344;
    out->chip_model = 9;  // esp32s3
    out->chip_rev = 2;
    out->idf_ver = "host";
    out->app_name = "core_test";
    out->app_build_time = "test";
    for (int i = 0; i < 8; i++) out->elf_sha[i] = static_cast<uint8_t>(i + 1);
    out->cpu_freq_mhz = 240;
    out->free_heap = 123456;
}

void reset_device() {
    g_resets.fetch_add(1, std::memory_order_relaxed);
}

}  // namespace port

// Controles del mock, usados por los tests.
namespace testhooks {
void set_time_us(uint64_t t) {
    port::g_fake_us.store(t, std::memory_order_relaxed);
}
void advance_us(uint64_t dt) {
    port::g_fake_us.fetch_add(dt, std::memory_order_relaxed);
}
uint32_t reset_count() {
    return port::g_resets.load(std::memory_order_relaxed);
}
}  // namespace testhooks

}  // namespace mole
