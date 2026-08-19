# protocol/

Artefactos compartidos del protocolo Mole v2 (spec §6/§7, congeladas en draft.8).

| Archivo | Qué es |
|---|---|
| `codec_vectors.json` | Set de vectores de test compartido entre Rust y C++ (se crea en T-12) |
| `codec_vectors.schema.json` | JSON Schema del formato de vectores (se crea en T-02) |
| `anchors.py` | Tercer camino independiente: recalcula los vectores ancla leyendo la spec, no el código (se crea en T-03) |

Los 12 vectores ancla, su cálculo paso a paso y la convención de CRC32 (Q-1) se documentan acá en T-03/T-05. Si alguna vez hay una discrepancia entre implementaciones, este documento es el árbitro.

`codec_vectors.json` se commitea y CI lo valida; no se regenera en CI.
