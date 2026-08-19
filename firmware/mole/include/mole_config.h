// SPDX-License-Identifier: MIT
// mole_config.h — defaults de compilación (FW-11..13). En ESP-IDF, los
// CONFIG_MOLE_* de Kconfig tienen prioridad; fuera de IDF valen los -D o
// estos defaults.
#pragma once

#if defined(__has_include)
#if __has_include("sdkconfig.h")
#include "sdkconfig.h"
#endif
#endif

// FW-12: MOLE_ENABLED=0 compila toda la instrumentación a nada.
// Los bools de Kconfig están definidos (=1) o ausentes: bajo IDF, la
// ausencia de CONFIG_MOLE_ENABLED significa "apagado por menuconfig";
// fuera de IDF el default es encendido.
#ifndef MOLE_ENABLED
#if defined(ESP_PLATFORM)
#ifdef CONFIG_MOLE_ENABLED
#define MOLE_ENABLED 1
#else
#define MOLE_ENABLED 0
#endif
#else
#define MOLE_ENABLED 1
#endif
#endif

#ifndef MOLE_MAX_SYMBOLS
#ifdef CONFIG_MOLE_MAX_SYMBOLS
#define MOLE_MAX_SYMBOLS CONFIG_MOLE_MAX_SYMBOLS
#else
#define MOLE_MAX_SYMBOLS 512
#endif
#endif

#ifndef MOLE_RING_SIZE
#ifdef CONFIG_MOLE_RING_SIZE
#define MOLE_RING_SIZE CONFIG_MOLE_RING_SIZE
#else
#define MOLE_RING_SIZE 4096
#endif
#endif

#ifndef MOLE_ISR_RING_SIZE
#ifdef CONFIG_MOLE_ISR_RING_SIZE
#define MOLE_ISR_RING_SIZE CONFIG_MOLE_ISR_RING_SIZE
#else
#define MOLE_ISR_RING_SIZE 1024
#endif
#endif

#ifndef MOLE_MAX_PRODUCERS
#ifdef CONFIG_MOLE_MAX_PRODUCERS
#define MOLE_MAX_PRODUCERS CONFIG_MOLE_MAX_PRODUCERS
#else
#define MOLE_MAX_PRODUCERS 8
#endif
#endif

#ifndef MOLE_FRAME_MAX
#ifdef CONFIG_MOLE_FRAME_MAX
#define MOLE_FRAME_MAX CONFIG_MOLE_FRAME_MAX
#else
#define MOLE_FRAME_MAX 4096
#endif
#endif

#ifndef MOLE_FLUSH_MS
#ifdef CONFIG_MOLE_FLUSH_MS
#define MOLE_FLUSH_MS CONFIG_MOLE_FLUSH_MS
#else
#define MOLE_FLUSH_MS 5
#endif
#endif

#ifndef MOLE_BLOCK_TIMEOUT_MS
#ifdef CONFIG_MOLE_BLOCK_TIMEOUT_MS
#define MOLE_BLOCK_TIMEOUT_MS CONFIG_MOLE_BLOCK_TIMEOUT_MS
#else
#define MOLE_BLOCK_TIMEOUT_MS 10
#endif
#endif

// FW-13: niveles y canales por compilación.
#ifndef MOLE_LEVEL_MIN
#define MOLE_LEVEL_MIN 0
#endif
#ifndef MOLE_LOG_SOURCE
#define MOLE_LOG_SOURCE 1
#endif
