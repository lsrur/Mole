// SPDX-License-Identifier: MIT
// Transporte USB-CDC nativo via TinyUSB (TR-02) — el preferido en S2/S3 y el
// que debe cumplir PERF-04. Requiere el managed component
// espressif/esp_tinyusb en la app; si no está, la fábrica devuelve nullptr y
// begin() cae a UART. Verificación en placa pendiente (F1-T08).

#ifdef ESP_PLATFORM

#include "mole_transport.h"

#if defined(__has_include)
#if __has_include("tinyusb.h") && __has_include("tusb_cdc_acm.h")
#define MOLE_HAS_TINYUSB 1
#endif
#endif

#ifdef MOLE_HAS_TINYUSB

#include "tinyusb.h"
#include "tusb.h"
#include "tusb_cdc_acm.h"

namespace mole {

namespace {

class CdcTransport final : public IMoleTransport {
  public:
    CdcTransport() {
        const tinyusb_config_t tusb_cfg = {};
        tinyusb_driver_install(&tusb_cfg);
        tinyusb_config_cdcacm_t acm_cfg = {};
        acm_cfg.usb_dev = TINYUSB_USBDEV_0;
        acm_cfg.cdc_port = TINYUSB_CDC_ACM_0;
        tusb_cdc_acm_init(&acm_cfg);
    }

    bool write(const uint8_t* data, size_t n) override {
        if (!connected()) return false;  // TR-08
        size_t queued = tinyusb_cdcacm_write_queue(TINYUSB_CDC_ACM_0, data, n);
        tinyusb_cdcacm_write_flush(TINYUSB_CDC_ACM_0, 0);
        return queued == n;
    }

    size_t read(uint8_t* buf, size_t cap) override {
        size_t rx = 0;
        if (tinyusb_cdcacm_read(TINYUSB_CDC_ACM_0, buf, cap, &rx) != ESP_OK) {
            return 0;
        }
        return rx;
    }

    bool connected() override {
        return tud_cdc_n_connected(TINYUSB_CDC_ACM_0);
    }
};

}  // namespace

IMoleTransport* transport_cdc() {
    static CdcTransport t;
    return &t;
}

}  // namespace mole

#else  // sin esp_tinyusb en la app

namespace mole {
IMoleTransport* transport_cdc() {
    return nullptr;
}
}  // namespace mole

#endif  // MOLE_HAS_TINYUSB
#endif  // ESP_PLATFORM
