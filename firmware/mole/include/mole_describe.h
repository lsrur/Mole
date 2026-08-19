// SPDX-License-Identifier: MIT
// mole_describe.h — descriptores de tipo no intrusivos (§7.2.4, REC-40..48,
// REC-53/54). MOLE_DESCRIBE genera una función libre constexpr hallada por
// ADL; el registro de wire (REC_TYPE_DEF/REC_ENUM_DEF) es perezoso.
// Incluido por mole.h; no usar directamente.
#pragma once

#include <cstddef>

#include "mole_log.h"

namespace mole {

// ---------------------------------------------------------------------------
// Descriptores en memoria (constexpr)
// ---------------------------------------------------------------------------

struct FieldDesc {
    const char* name;
    uint8_t wire;
    uint8_t flags;  // kFieldFlagBE / kFieldFlagArray / kFieldFlagEnum
    uint16_t offset;
    uint16_t size;
    // para struct anidado o enum descripto: registra (si hace falta) y
    // devuelve su type_id. nullptr para escalares.
    uint16_t (*ref_type_id)();
    // para struct anidado: empaqueta el campo en modo valores. nullptr si
    // el campo se copia plano (escalares, arreglos, BE crudo).
    void (*pack_fn)(detail::PackBuf&, const void*);
};

template <size_t N>
struct TypeDesc {
    const char* name;
    FieldDesc fields[N];
    static constexpr size_t count = N;
};

struct EnumEntryDesc {
    const char* name;
    int64_t value;
};

template <size_t N>
struct EnumDesc {
    const char* name;
    uint8_t wire;  // entero base (REC-53)
    EnumEntryDesc entries[N];
    static constexpr size_t count = N;
};

namespace detail {

// ---- API del core (implementada en mole_core.cpp) ----
uint16_t alloc_type_id();
bool meta_push_public(uint8_t type, const uint8_t* payload, uint8_t len);

// (HasTypeDesc/HasEnumDesc y las forward declarations de type_id/
// pack_struct_values viven en mole_log.h)

template <class T>
uint16_t register_type() {
    constexpr auto d = mole_describe(static_cast<const T*>(nullptr));
    const uint16_t id = alloc_type_id();
    uint8_t payload[255];
    uint8_t p = 0;
    payload[p++] = static_cast<uint8_t>(id);
    payload[p++] = static_cast<uint8_t>(id >> 8);
    size_t nlen = std::strlen(d.name);
    if (nlen > 80) nlen = 80;
    payload[p++] = static_cast<uint8_t>(nlen);
    std::memcpy(payload + p, d.name, nlen);
    p = static_cast<uint8_t>(p + nlen);
    payload[p++] = static_cast<uint8_t>(d.count);
    for (size_t i = 0; i < d.count; i++) {
        const FieldDesc& f = d.fields[i];
        // los anidados/enums se registran (y emiten) ANTES que este payload
        const uint16_t ref = f.ref_type_id ? f.ref_type_id() : 0;
        const SymId name_sym = intern(f.name, static_cast<SymKind>(0), 0);
        payload[p++] = static_cast<uint8_t>(name_sym);
        payload[p++] = static_cast<uint8_t>(name_sym >> 8);
        payload[p++] = f.wire;
        payload[p++] = f.flags;
        payload[p++] = static_cast<uint8_t>(f.offset);
        payload[p++] = static_cast<uint8_t>(f.offset >> 8);
        payload[p++] = static_cast<uint8_t>(f.size);
        payload[p++] = static_cast<uint8_t>(f.size >> 8);
        payload[p++] = static_cast<uint8_t>(ref);
        payload[p++] = static_cast<uint8_t>(ref >> 8);
    }
    meta_push_public(REC_TYPE_DEF, payload, p);
    return id;
}

template <class E>
uint16_t register_enum() {
    constexpr auto d = mole_describe_enum(static_cast<const E*>(nullptr));
    const uint16_t id = alloc_type_id();
    const size_t esize = wire_fixed_size(d.wire);
    uint8_t payload[255];
    uint8_t p = 0;
    payload[p++] = static_cast<uint8_t>(id);
    payload[p++] = static_cast<uint8_t>(id >> 8);
    size_t nlen = std::strlen(d.name);
    if (nlen > 80) nlen = 80;
    payload[p++] = static_cast<uint8_t>(nlen);
    std::memcpy(payload + p, d.name, nlen);
    p = static_cast<uint8_t>(p + nlen);
    payload[p++] = d.wire;
    payload[p++] = static_cast<uint8_t>(d.count);
    for (size_t i = 0; i < d.count; i++) {
        const uint64_t v = static_cast<uint64_t>(d.entries[i].value);
        for (size_t k = 0; k < esize; k++) {
            payload[p++] = static_cast<uint8_t>(v >> (8 * k));
        }
        const SymId name_sym = intern(d.entries[i].name, static_cast<SymKind>(0), 0);
        payload[p++] = static_cast<uint8_t>(name_sym);
        payload[p++] = static_cast<uint8_t>(name_sym >> 8);
    }
    meta_push_public(REC_ENUM_DEF, payload, p);
    return id;
}

template <class T>
uint16_t type_id() {
    static const uint16_t id = register_type<T>();
    return id;
}

template <class E>
uint16_t enum_type_id() {
    static const uint16_t id = register_enum<E>();
    return id;
}

// ---- construcción de FieldDesc (usada por las macros) ----

template <class Inner>
void pack_nested(PackBuf& b, const void* p) {
    pack_struct_values<Inner>(b, *static_cast<const Inner*>(p));
}

template <class FT>
constexpr FieldDesc make_field(const char* name, size_t off, uint8_t extra_flags) {
    if constexpr (std::is_array_v<FT>) {
        // REC-54: arreglo detectado por el tipo; solo elementos escalares
        using Elem = std::remove_cv_t<std::remove_extent_t<FT>>;
        static_assert(std::is_arithmetic_v<Elem>,
                      "MOLE: arreglo de no-escalares no soportado (REC-54)");
        return {name, scalar_wire<Elem>(),
                static_cast<uint8_t>(kFieldFlagArray | extra_flags),
                static_cast<uint16_t>(off), static_cast<uint16_t>(sizeof(FT)),
                nullptr, nullptr};
    } else if constexpr (std::is_enum_v<FT>) {
        using U = std::underlying_type_t<FT>;
        if constexpr (HasEnumDesc<FT>::value) {
            return {name, scalar_wire<U>(),
                    static_cast<uint8_t>(kFieldFlagEnum | extra_flags),
                    static_cast<uint16_t>(off), static_cast<uint16_t>(sizeof(FT)),
                    &enum_type_id<FT>, nullptr};
        } else {
            // sin descriptor: viaja como su entero base (REC-41)
            return {name, scalar_wire<U>(), extra_flags,
                    static_cast<uint16_t>(off), static_cast<uint16_t>(sizeof(FT)),
                    nullptr, nullptr};
        }
    } else if constexpr (HasTypeDesc<FT>::value) {
        return {name, WIRE_STRUCT, extra_flags, static_cast<uint16_t>(off),
                static_cast<uint16_t>(sizeof(FT)), &type_id<FT>, &pack_nested<FT>};
    } else if constexpr (is_char_ptr_v<FT>) {
        // cadena de runtime adentro del struct (REC-46): decodificación
        // secuencial en el host
        return {name, WIRE_STR, extra_flags, static_cast<uint16_t>(off),
                static_cast<uint16_t>(sizeof(FT)), nullptr, nullptr};
    } else {
        return {name, scalar_wire<FT>(), extra_flags, static_cast<uint16_t>(off),
                static_cast<uint16_t>(sizeof(FT)), nullptr, nullptr};
    }
}

// ---- empaquetado modo valores (REC-45): secuencial, sin padding, bytes
// tal como están en memoria (el swap BE lo hace el host, Q-2) ----
template <class T>
void pack_struct_values(PackBuf& b, const T& obj) {
    constexpr auto d = mole_describe(static_cast<const T*>(nullptr));
    const uint8_t* base = reinterpret_cast<const uint8_t*>(&obj);
    for (size_t i = 0; i < d.count; i++) {
        const FieldDesc& f = d.fields[i];
        const uint8_t* fp = base + f.offset;
        if (f.pack_fn) {
            f.pack_fn(b, fp);
        } else if (f.wire == WIRE_STR) {
            const char* s = *reinterpret_cast<const char* const*>(fp);
            size_t n = s ? std::strlen(s) : 0;
            if (n > 255) n = 255;
            b.u8(static_cast<uint8_t>(n));
            b.put(s, n);
        } else {
            b.put(fp, f.size);
        }
    }
}

// ---- hexdump inline sin descriptor (REC-48) ----
struct Bytes {
    const void* ptr;
    size_t len;
};

inline void pack_arg(PackBuf& b, const Bytes& v) {
    static const char* kHex = "0123456789abcdef";
    const uint8_t* p = static_cast<const uint8_t*>(v.ptr);
    size_t n = v.len;
    bool trunc = false;
    if (n > 126) {
        n = 126;  // 252 chars + "…" marker; nunca truncado en silencio
        trunc = true;
    }
    b.u8(static_cast<uint8_t>(2 * n + (trunc ? 1 : 0)));
    for (size_t i = 0; i < n; i++) {
        b.u8(static_cast<uint8_t>(kHex[p[i] >> 4]));
        b.u8(static_cast<uint8_t>(kHex[p[i] & 0xF]));
    }
    if (trunc) b.u8('+');
}

}  // namespace detail
}  // namespace mole

// ---------------------------------------------------------------------------
// Macros MOLE_DESCRIBE / _BE / _ENUM (hasta 16 campos)
// ---------------------------------------------------------------------------

#define MOLE_D_GET17_(_1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, \
                      _14, _15, _16, NAME, ...) NAME

#define MOLE_D_FE1_(F, T, a) F(T, a)
#define MOLE_D_FE2_(F, T, a, ...) F(T, a), MOLE_D_FE1_(F, T, __VA_ARGS__)
#define MOLE_D_FE3_(F, T, a, ...) F(T, a), MOLE_D_FE2_(F, T, __VA_ARGS__)
#define MOLE_D_FE4_(F, T, a, ...) F(T, a), MOLE_D_FE3_(F, T, __VA_ARGS__)
#define MOLE_D_FE5_(F, T, a, ...) F(T, a), MOLE_D_FE4_(F, T, __VA_ARGS__)
#define MOLE_D_FE6_(F, T, a, ...) F(T, a), MOLE_D_FE5_(F, T, __VA_ARGS__)
#define MOLE_D_FE7_(F, T, a, ...) F(T, a), MOLE_D_FE6_(F, T, __VA_ARGS__)
#define MOLE_D_FE8_(F, T, a, ...) F(T, a), MOLE_D_FE7_(F, T, __VA_ARGS__)
#define MOLE_D_FE9_(F, T, a, ...) F(T, a), MOLE_D_FE8_(F, T, __VA_ARGS__)
#define MOLE_D_FE10_(F, T, a, ...) F(T, a), MOLE_D_FE9_(F, T, __VA_ARGS__)
#define MOLE_D_FE11_(F, T, a, ...) F(T, a), MOLE_D_FE10_(F, T, __VA_ARGS__)
#define MOLE_D_FE12_(F, T, a, ...) F(T, a), MOLE_D_FE11_(F, T, __VA_ARGS__)
#define MOLE_D_FE13_(F, T, a, ...) F(T, a), MOLE_D_FE12_(F, T, __VA_ARGS__)
#define MOLE_D_FE14_(F, T, a, ...) F(T, a), MOLE_D_FE13_(F, T, __VA_ARGS__)
#define MOLE_D_FE15_(F, T, a, ...) F(T, a), MOLE_D_FE14_(F, T, __VA_ARGS__)
#define MOLE_D_FE16_(F, T, a, ...) F(T, a), MOLE_D_FE15_(F, T, __VA_ARGS__)

#define MOLE_D_FOR_EACH_(F, T, ...)                                          \
    MOLE_D_GET17_(__VA_ARGS__, MOLE_D_FE16_, MOLE_D_FE15_, MOLE_D_FE14_,     \
                  MOLE_D_FE13_, MOLE_D_FE12_, MOLE_D_FE11_, MOLE_D_FE10_,    \
                  MOLE_D_FE9_, MOLE_D_FE8_, MOLE_D_FE7_, MOLE_D_FE6_,        \
                  MOLE_D_FE5_, MOLE_D_FE4_, MOLE_D_FE3_, MOLE_D_FE2_,        \
                  MOLE_D_FE1_)(F, T, __VA_ARGS__)

#define MOLE_D_NFIELDS_(...)                                                  \
    MOLE_D_GET17_(__VA_ARGS__, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, \
                  3, 2, 1)

#define MOLE_D_FIELD_(T, f) \
    ::mole::detail::make_field<decltype(T::f)>(#f, offsetof(T, f), 0)
#define MOLE_D_FIELD_BE_(T, f) \
    ::mole::detail::make_field<decltype(T::f)>(#f, offsetof(T, f), ::mole::kFieldFlagBE)

// Descriptor no intrusivo, al lado del struct (REC-40/REC-42).
#define MOLE_DESCRIBE(T, ...)                                                 \
    inline constexpr auto mole_describe(const T*) {                           \
        return ::mole::TypeDesc<MOLE_D_NFIELDS_(__VA_ARGS__)>{                \
            #T, {MOLE_D_FOR_EACH_(MOLE_D_FIELD_, T, __VA_ARGS__)}};           \
    }

// Variante con todos los campos big-endian (registros de sensor, REC-16).
#define MOLE_DESCRIBE_BE(T, ...)                                              \
    inline constexpr auto mole_describe(const T*) {                           \
        return ::mole::TypeDesc<MOLE_D_NFIELDS_(__VA_ARGS__)>{                \
            #T, {MOLE_D_FOR_EACH_(MOLE_D_FIELD_BE_, T, __VA_ARGS__)}};        \
    }

#define MOLE_D_ENUM_ENTRY_(E, v) \
    ::mole::EnumEntryDesc{#v, static_cast<int64_t>(E::v)}

// Valores por nombre (REC-41/REC-53). La definición DEBE caber en un record.
#define MOLE_DESCRIBE_ENUM(E, ...)                                            \
    inline constexpr auto mole_describe_enum(const E*) {                      \
        static_assert(MOLE_D_NFIELDS_(__VA_ARGS__) <= 40,                     \
                      "MOLE: el enum no entra en un REC_ENUM_DEF (REC-53)");  \
        return ::mole::EnumDesc<MOLE_D_NFIELDS_(__VA_ARGS__)>{                \
            #E, ::mole::detail::scalar_wire<std::underlying_type_t<E>>(),     \
            {MOLE_D_FOR_EACH_(MOLE_D_ENUM_ENTRY_, E, __VA_ARGS__)}};          \
    }

// Vista de bytes sin descriptor (REC-48): hexdump inline degradado.
#define MOLE_BYTES(obj) \
    ::mole::detail::Bytes{&(obj), sizeof(obj)}

// TypeId del descriptor de T (para mole::dump en F4).
#define MOLE_TYPE(T) ::mole::detail::type_id<T>()
