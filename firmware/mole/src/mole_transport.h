// SPDX-License-Identifier: MIT
// mole_transport.h — la única abstracción real del firmware (TR-07):
// hay tres transportes desde el día uno. Todo lo demás llama a IDF directo.
#pragma once

#include <stddef.h>
#include <stdint.h>

namespace mole {

class IMoleTransport {
  public:
    virtual ~IMoleTransport() = default;
    // Escribe el frame completo; devuelve false si el enlace no está.
    virtual bool write(const uint8_t* data, size_t n) = 0;
    // Lee lo que haya (no bloqueante); devuelve bytes leídos.
    virtual size_t read(uint8_t* buf, size_t cap) = 0;
    // TR-08: sin host conectado, el productor descarta gratis.
    virtual bool connected() = 0;
};

// Fábricas (devuelven nullptr si el transporte no existe en este target).
IMoleTransport* transport_uart(uint32_t baud);
IMoleTransport* transport_cdc();

}  // namespace mole
