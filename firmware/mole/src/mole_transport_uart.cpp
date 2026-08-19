// SPDX-License-Identifier: MIT
// Transporte UART (TR-01): UART0, hasta 2 Mbaud. Verificación en placa
// pendiente (F1-T08, frente hardware).

#ifdef ESP_PLATFORM

#include "driver/uart.h"
#include "mole_config.h"
#include "mole_transport.h"

namespace mole {

namespace {

class UartTransport final : public IMoleTransport {
  public:
    explicit UartTransport(uint32_t baud) {
        uart_config_t cfg = {};
        cfg.baud_rate = static_cast<int>(baud);
        cfg.data_bits = UART_DATA_8_BITS;
        cfg.parity = UART_PARITY_DISABLE;
        cfg.stop_bits = UART_STOP_BITS_1;
        cfg.flow_ctrl = UART_HW_FLOWCTRL_DISABLE;
        cfg.source_clk = UART_SCLK_DEFAULT;
        uart_driver_install(UART_NUM_0, 2048, MOLE_FRAME_MAX * 2, 0, nullptr, 0);
        uart_param_config(UART_NUM_0, &cfg);
    }

    bool write(const uint8_t* data, size_t n) override {
        return uart_write_bytes(UART_NUM_0, data, n) == static_cast<int>(n);
    }

    size_t read(uint8_t* buf, size_t cap) override {
        const int r = uart_read_bytes(UART_NUM_0, buf, cap, 0);
        return r > 0 ? static_cast<size_t>(r) : 0;
    }

    bool connected() override {
        return true;  // un UART no sabe si hay alguien del otro lado
    }
};

}  // namespace

IMoleTransport* transport_uart(uint32_t baud) {
    static UartTransport t{baud};
    return &t;
}

}  // namespace mole

#endif  // ESP_PLATFORM
