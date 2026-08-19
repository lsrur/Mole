// SPDX-License-Identifier: MIT
// mole_ring.h — ring buffer SPSC lock-free (FW-04). Un productor (la tarea
// dueña del slot) y un consumidor (la moleTask). Sin locks en el hot path:
// head/tail atómicos con acquire/release.
//
// Entrada en el ring: [total u16][t_us u64][type u8][len u8][payload]
#pragma once

#include <atomic>
#include <cstdint>
#include <cstring>

namespace mole {
namespace detail {

constexpr size_t kRingEntryHeader = 2 + 8 + 1 + 1;

struct RingView {
    uint8_t type = 0;
    uint64_t t_us = 0;
    const uint8_t* payload = nullptr;
    uint8_t len = 0;
};

class Ring {
  public:
    // La memoria la provee el dueño (estática o heap en begin()); el ring
    // nunca aloca en el hot path. `size` DEBE ser potencia de dos: los
    // índices son u32 monotónicos y la posición real es `idx & (size-1)`,
    // que solo es continua en el wrap de u32 si size divide a 2^32.
    void init(uint8_t* storage, uint32_t size) {
        buf_ = ((size & (size - 1)) == 0 && size != 0) ? storage : nullptr;
        size_ = size;
        mask_ = size - 1;
        head_.store(0, std::memory_order_relaxed);
        tail_.store(0, std::memory_order_relaxed);
        high_water_ = 0;
    }

    // Vuelve al estado sin inicializar (tests). La memoria previa se pierde:
    // solo se usa en host.
    void reset() {
        buf_ = nullptr;
        size_ = 0;
        mask_ = 0;
        head_.store(0, std::memory_order_relaxed);
        tail_.store(0, std::memory_order_relaxed);
        high_water_ = 0;
    }

    bool valid() const { return buf_ != nullptr; }
    uint32_t size() const { return size_; }

    uint32_t used() const {
        return tail_.load(std::memory_order_acquire) -
               head_.load(std::memory_order_acquire);
    }

    uint16_t high_water() const { return high_water_; }

    // Lado productor. false = no entra (el llamador aplica la política).
    bool push(uint8_t type, uint64_t t_us, const uint8_t* payload, uint8_t len) {
        const uint32_t total = static_cast<uint32_t>(kRingEntryHeader) + len;
        const uint32_t head = head_.load(std::memory_order_acquire);
        const uint32_t tail = tail_.load(std::memory_order_relaxed);
        const uint32_t used = tail - head;
        if (used + total > size_) {
            return false;
        }
        uint8_t hdr[kRingEntryHeader];
        hdr[0] = static_cast<uint8_t>(total);
        hdr[1] = static_cast<uint8_t>(total >> 8);
        std::memcpy(hdr + 2, &t_us, 8);
        hdr[10] = type;
        hdr[11] = len;
        write_at(tail, hdr, kRingEntryHeader);
        write_at(tail + kRingEntryHeader, payload, len);
        tail_.store(tail + total, std::memory_order_release);
        const uint32_t now_used = used + total;
        if (now_used > high_water_) {
            high_water_ = static_cast<uint16_t>(now_used > 0xFFFF ? 0xFFFF : now_used);
        }
        return true;
    }

    // Lado consumidor. Copia el payload a buf (≥255 bytes).
    bool pop(RingView* out, uint8_t* buf) {
        const uint32_t tail = tail_.load(std::memory_order_acquire);
        const uint32_t head = head_.load(std::memory_order_relaxed);
        if (tail == head) return false;
        uint8_t hdr[kRingEntryHeader];
        read_at(head, hdr, kRingEntryHeader);
        const uint16_t total = static_cast<uint16_t>(hdr[0] | (hdr[1] << 8));
        std::memcpy(&out->t_us, hdr + 2, 8);
        out->type = hdr[10];
        out->len = hdr[11];
        read_at(head + kRingEntryHeader, buf, out->len);
        out->payload = buf;
        head_.store(head + total, std::memory_order_release);
        return true;
    }

    // ¿El próximo record entra? (para que la moleTask decida cortar frame)
    bool peek_len(uint8_t* type, uint8_t* len) const {
        const uint32_t tail = tail_.load(std::memory_order_acquire);
        const uint32_t head = head_.load(std::memory_order_relaxed);
        if (tail == head) return false;
        *type = at(head + 10);
        *len = at(head + 11);
        return true;
    }

  private:
    uint8_t at(uint32_t pos) const { return buf_[pos & mask_]; }

    void write_at(uint32_t pos, const uint8_t* src, uint32_t n) {
        const uint32_t off = pos & mask_;
        const uint32_t first = (off + n <= size_) ? n : size_ - off;
        std::memcpy(buf_ + off, src, first);
        if (first < n) std::memcpy(buf_, src + first, n - first);
    }

    void read_at(uint32_t pos, uint8_t* dst, uint32_t n) {
        const uint32_t off = pos & mask_;
        const uint32_t first = (off + n <= size_) ? n : size_ - off;
        std::memcpy(dst, buf_ + off, first);
        if (first < n) std::memcpy(dst + first, buf_, n - first);
    }

    uint8_t* buf_ = nullptr;
    uint32_t size_ = 0;
    uint32_t mask_ = 0;
    std::atomic<uint32_t> head_{0};
    std::atomic<uint32_t> tail_{0};
    uint16_t high_water_ = 0;
};

}  // namespace detail
}  // namespace mole
