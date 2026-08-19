# Resultados de medición — F1

Placa de referencia: ESP32-S3 · IDF v5.5.1 · builds con `sdkconfig.defaults`.

## PERF-15 — flash ahorrado por el formateo diferido (F1-T10)

Metodología (M-6 del plan): dos builds del bench, ambos con el componente
Mole completo linkeado y un log de float por `MOLE_INFO`. El de control
agrega un `printf("%f")`, que bajo `CONFIG_NEWLIB_NANO_FORMAT=y` no existe:
obliga a desactivar nano-format y linkear el formateo completo de newlib.

| Build | newlib | `mole_bench.bin` |
|---|---|---|
| Diferido (solo Mole) | nano-format | 203.856 B |
| Control (`printf("%f")`) | full-format | 224.208 B |

**Delta: 20.352 bytes de flash.** Objetivo de la spec: ≥ 4096 B → **cumplido
(5×)**. La lectura correcta: el formateo diferido permite quedarse en
nano-format aunque el firmware loguee floats; un solo `%f` en el código lo
impide.

Nota: con la config default de IDF (newlib full), el delta es ~16 B porque
el soporte de float ya está linkeado de entrada — la comparación relevante
es contra la config que un release cuidadoso usaría.

Reproducción:

```sh
idf.py build
idf.py -B build_ctrl -DSDKCONFIG=$PWD/build_ctrl/sdkconfig \
  -DSDKCONFIG_DEFAULTS="$PWD/sdkconfig.defaults;$PWD/sdkconfig.ctrl" \
  -DCMAKE_CXX_FLAGS=-DBENCH_CONTROL_PRINTF build
```

## Pendientes de hardware (bloqueados hasta tener la placa)

PERF-01/02/14 (ciclos), PERF-04/05/06/07 (throughput con `molectl bench`),
PERF-12 (RAM con `heap_caps_get_info`). PERF-03 pendiente formal (sin
instrumento, Q-4 del plan).
