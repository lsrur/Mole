// SPDX-License-Identifier: MIT
// mole_log.h — maquinaria de las macros de log diferido (§7.2, REC-32..38).
// El MCU nunca formatea: empaqueta el fmt_id y los argumentos en binario.
// Incluido por mole.h; no usar directamente.
#pragma once

#include <atomic>
#include <cstring>
#include <type_traits>

#include "mole_config.h"
#include "mole_wire.h"

namespace mole {

enum class Level : uint8_t {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
};

using SymId = uint16_t;

SymId intern(const char* name, SymKind kind, SymId parent);

namespace port {
uint8_t task_slot();
uint8_t core_id();
}  // namespace port

namespace detail {

// ---- API del core que consumen los sitios de log (mole_core.cpp) ----
bool log_enabled(uint8_t level, SymId tag);
uint16_t register_fmt(const char* fmt, const char* file, uint16_t line,
                      uint8_t argc, const uint8_t* tags, uint8_t tags_len);
bool ring_push(uint8_t rec_type, const uint8_t* payload, uint8_t len);
void count_oversize_log();

// ---- conteo de llaves en compilación (REC-33) ----
// `{}`/`{:...}` cuentan; `{{` y `}}` son escapes.
constexpr size_t count_holes(const char* s) {
    size_t n = 0;
    for (size_t i = 0; s[i] != '\0'; i++) {
        if (s[i] == '{') {
            if (s[i + 1] == '{') {
                i++;
                continue;
            }
            n++;
            while (s[i] != '\0' && s[i] != '}') i++;
            if (s[i] == '\0') break;
        } else if (s[i] == '}' && s[i + 1] == '}') {
            i++;
        }
    }
    return n;
}

// sizeof...(args) evaluable en static_assert vía decltype (contexto no
// evaluado, funciona con valores de runtime).
template <class... A>
constexpr std::integral_constant<size_t, sizeof...(A)> arg_count(const A&...) {
    return {};
}

// ---- mapeo tipo C++ → tag de wire (REC-52). Un tipo sin mapeo es error
// de compilación, no conversión silenciosa (REC-37). ----

template <class T>
constexpr uint8_t scalar_wire() {
    using U = std::remove_cv_t<T>;
    if constexpr (std::is_same_v<U, bool>) {
        return WIRE_BOOL;
    } else if constexpr (std::is_floating_point_v<U>) {
        static_assert(sizeof(U) == 4 || sizeof(U) == 8, "float no soportado");
        return sizeof(U) == 4 ? WIRE_F32 : WIRE_F64;
    } else if constexpr (std::is_integral_v<U>) {
        static_assert(sizeof(U) <= 8, "entero no soportado");
        constexpr bool s = std::is_signed_v<U>;
        switch (sizeof(U)) {
            case 1: return s ? WIRE_I8 : WIRE_U8;
            case 2: return s ? WIRE_I16 : WIRE_U16;
            case 4: return s ? WIRE_I32 : WIRE_U32;
            default: return s ? WIRE_I64 : WIRE_U64;
        }
    } else if constexpr (std::is_pointer_v<U>) {
        return WIRE_PTR;  // dirección; cadenas tienen sobrecarga propia
    } else {
        static_assert(!sizeof(U*), "MOLE: tipo de argumento no soportado (REC-37)");
        return 0;
    }
}

// ---- buffer de empaquetado en stack, tope 255 (PR-07/PR-09) ----
struct PackBuf {
    uint8_t data[255];
    uint8_t len = 0;
    bool overflow = false;

    void put(const void* p, size_t n) {
        if (static_cast<size_t>(len) + n > sizeof data) {
            overflow = true;
            return;
        }
        std::memcpy(data + len, p, n);
        len = static_cast<uint8_t>(len + n);
    }
    void u8(uint8_t v) { put(&v, 1); }
    void u16(uint16_t v) {
        const uint8_t b[2] = {static_cast<uint8_t>(v), static_cast<uint8_t>(v >> 8)};
        put(b, 2);
    }
};

// ---- empaquetado de argumentos (REC-52, REC-38) ----
//
// Un solo template con if-constexpr: la resolución por sobrecargas es una
// trampa acá, porque el decay array→puntero cuenta como "exact match" y una
// sobrecarga const char* le gana al template de char[N], convirtiendo
// literales en copias de runtime en silencio.

template <class T>
constexpr bool is_char_array_v =
    std::is_array_v<T> &&
    std::is_same_v<std::remove_cv_t<std::remove_extent_t<T>>, char>;

template <class T>
constexpr bool is_char_ptr_v =
    std::is_same_v<std::remove_cv_t<T>, const char*> ||
    std::is_same_v<std::remove_cv_t<T>, char*>;

// ---- detección de descriptores por ADL (mole_describe.h) ----
// La búsqueda es puramente ADL: si el tipo tiene un mole_describe/-_enum
// al lado (REC-40), se encuentra en el contexto de instanciación.
template <class T, class = void>
struct HasTypeDesc : std::false_type {};
template <class T>
struct HasTypeDesc<T, std::void_t<decltype(mole_describe(static_cast<const T*>(nullptr)))>>
    : std::true_type {};

template <class T, class = void>
struct HasEnumDesc : std::false_type {};
template <class T>
struct HasEnumDesc<T, std::void_t<decltype(mole_describe_enum(static_cast<const T*>(nullptr)))>>
    : std::true_type {};

// definidos en mole_describe.h; visibles en el punto de instanciación
template <class T>
void pack_struct_values(PackBuf& b, const T& obj);
template <class T>
uint16_t type_id();
template <class E>
uint16_t enum_type_id();
struct Bytes;

template <class T>
inline void pack_arg(PackBuf& b, const T& v) {
    if constexpr (is_char_array_v<T>) {
        // literal: se interna, 2 bytes para siempre (REC-38). Kind 0 =
        // símbolo genérico.
        b.u16(intern(v, static_cast<SymKind>(0), 0));
    } else if constexpr (is_char_ptr_v<T>) {
        // cadena de runtime: copia inline con prefijo PR-20 (REC-38)
        size_t n = v ? std::strlen(v) : 0;
        if (n > 255) n = 255;
        b.u8(static_cast<uint8_t>(n));
        b.put(v, n);
    } else if constexpr (std::is_pointer_v<T>) {
        // dirección de 4 bytes (WIRE_PTR); en host de 64 bits se trunca
        const uintptr_t a = reinterpret_cast<uintptr_t>(v);
        b.u16(static_cast<uint16_t>(a));
        b.u16(static_cast<uint16_t>(a >> 16));
    } else if constexpr (std::is_same_v<std::remove_cv_t<T>, bool>) {
        b.u8(v ? 1 : 0);
    } else if constexpr (std::is_enum_v<std::remove_cv_t<T>>) {
        // con o sin descriptor viaja el entero base (REC-53); el nombre lo
        // resuelve el host
        b.put(&v, sizeof v);
    } else if constexpr (std::is_arithmetic_v<std::remove_cv_t<T>>) {
        // little-endian nativo en Xtensa/RISC-V y en los hosts soportados
        b.put(&v, sizeof v);
    } else if constexpr (HasTypeDesc<std::remove_cv_t<T>>::value) {
        // struct descripto: modo valores (REC-44/45)
        pack_struct_values(b, v);
    } else {
        static_assert(!sizeof(T*), "MOLE: tipo de argumento no soportado (REC-37)");
    }
}

// ---- tag de wire por argumento (para el REC_FMT_DEF); mismas ramas ----
template <class T>
inline void append_tag(uint8_t* tags, uint8_t& n) {
    using U = std::remove_cv_t<T>;
    if constexpr (is_char_array_v<T>) {
        tags[n++] = WIRE_SYM;
    } else if constexpr (is_char_ptr_v<T> || std::is_same_v<U, Bytes>) {
        // MOLE_BYTES viaja como cadena de runtime (hexdump degradado)
        tags[n++] = WIRE_STR;
    } else if constexpr (std::is_enum_v<U>) {
        if constexpr (HasEnumDesc<U>::value) {
            // 0xF1 + type_id inline (REC-53); registra si hace falta
            const uint16_t id = enum_type_id<U>();
            tags[n++] = kArgTagEnum;
            tags[n++] = static_cast<uint8_t>(id);
            tags[n++] = static_cast<uint8_t>(id >> 8);
        } else {
            tags[n++] = scalar_wire<std::underlying_type_t<U>>();
        }
    } else if constexpr (HasTypeDesc<U>::value) {
        // 0xF0 + type_id inline (REC-44); registra si hace falta
        const uint16_t id = type_id<U>();
        tags[n++] = WIRE_STRUCT;
        tags[n++] = static_cast<uint8_t>(id);
        tags[n++] = static_cast<uint8_t>(id >> 8);
    } else {
        tags[n++] = scalar_wire<T>();
    }
}

// ---- el sitio de log: registro perezoso + empaquetado + encolado ----
template <class... Args>
inline void log_site(std::atomic<uint16_t>* fid, Level lvl, SymId tag,
                     const char* fmt, const char* file, uint16_t line,
                     const Args&... args) {
    // el filtro corre ANTES de empaquetar (REC-47, PR-19)
    if (!log_enabled(static_cast<uint8_t>(lvl), tag)) return;

    uint16_t id = fid->load(std::memory_order_relaxed);
    if (id == 0) {
        // primera ejecución del sitio: REC_FMT_DEF (REC-34)
        uint8_t tags[3 * sizeof...(Args) + 1];
        uint8_t tn = 0;
        (append_tag<Args>(tags, tn), ...);
        const uint16_t fresh =
            register_fmt(fmt, file, line, static_cast<uint8_t>(sizeof...(Args)), tags, tn);
        uint16_t expected = 0;
        if (fid->compare_exchange_strong(expected, fresh)) {
            id = fresh;
        } else {
            id = expected;  // otro hilo ganó; su def ya viajó
        }
    }

    PackBuf b;
    b.u8(static_cast<uint8_t>(lvl));
    b.u8(port::task_slot());
    b.u8(port::core_id());
    b.u16(tag);
    b.u16(id);
    (pack_arg(b, args), ...);
    if (b.overflow) {
        count_oversize_log();  // nunca truncado en silencio
        return;
    }
    ring_push(REC_LOG_FMT, b.data, b.len);
}

}  // namespace detail
}  // namespace mole

// ---------------------------------------------------------------------------
// Macros públicas (REC-32/REC-33). El fmt DEBE ser literal: la concatenación
// `fmt ""` lo verifica en compilación.
// ---------------------------------------------------------------------------

#if MOLE_ENABLED

#define MOLE_LOG_AT_(lvl, tag_sym, fmtstr, ...)                                   \
    do {                                                                          \
        static_assert(                                                            \
            ::mole::detail::count_holes(fmtstr "") ==                             \
                decltype(::mole::detail::arg_count(__VA_ARGS__))::value,          \
            "MOLE: la cantidad de {} no coincide con los argumentos (REC-33)");   \
        static ::std::atomic<uint16_t> _mole_fid{0};                              \
        ::mole::detail::log_site(&_mole_fid, (lvl), (tag_sym), fmtstr "",         \
                                 __FILE__, static_cast<uint16_t>(__LINE__),       \
                                 ##__VA_ARGS__);                                  \
    } while (0)

// tag por identificador pelado, como en la spec: MOLE_WARN_T(net, "...")
#define MOLE_TAG_(tag)                                                          \
    ([]() -> ::mole::SymId {                                                    \
        static const ::mole::SymId _s = ::mole::intern(#tag, ::mole::KIND_TAG, 0); \
        return _s;                                                              \
    }())

#define MOLE_TRACE(fmt, ...) MOLE_LOG_AT_(::mole::Level::Trace, 0, fmt, ##__VA_ARGS__)
#define MOLE_DEBUG(fmt, ...) MOLE_LOG_AT_(::mole::Level::Debug, 0, fmt, ##__VA_ARGS__)
#define MOLE_INFO(fmt, ...) MOLE_LOG_AT_(::mole::Level::Info, 0, fmt, ##__VA_ARGS__)
#define MOLE_WARN(fmt, ...) MOLE_LOG_AT_(::mole::Level::Warn, 0, fmt, ##__VA_ARGS__)
#define MOLE_ERROR(fmt, ...) MOLE_LOG_AT_(::mole::Level::Error, 0, fmt, ##__VA_ARGS__)
#define MOLE_FATAL(fmt, ...) MOLE_LOG_AT_(::mole::Level::Fatal, 0, fmt, ##__VA_ARGS__)

#define MOLE_TRACE_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Trace, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)
#define MOLE_DEBUG_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Debug, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)
#define MOLE_INFO_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Info, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)
#define MOLE_WARN_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Warn, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)
#define MOLE_ERROR_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Error, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)
#define MOLE_FATAL_T(tag, fmt, ...) MOLE_LOG_AT_(::mole::Level::Fatal, MOLE_TAG_(tag), fmt, ##__VA_ARGS__)

#else  // !MOLE_ENABLED — FW-12

#define MOLE_TRACE(...) ((void)0)
#define MOLE_DEBUG(...) ((void)0)
#define MOLE_INFO(...) ((void)0)
#define MOLE_WARN(...) ((void)0)
#define MOLE_ERROR(...) ((void)0)
#define MOLE_FATAL(...) ((void)0)
#define MOLE_TRACE_T(...) ((void)0)
#define MOLE_DEBUG_T(...) ((void)0)
#define MOLE_INFO_T(...) ((void)0)
#define MOLE_WARN_T(...) ((void)0)
#define MOLE_ERROR_T(...) ((void)0)
#define MOLE_FATAL_T(...) ((void)0)

#endif
