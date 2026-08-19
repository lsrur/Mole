//! Filtrado de logs — HOST-09, FEAT-02: las columnas de enteros se
//! resuelven ANTES de tocar el texto; el render (mole-fmt) solo corre
//! sobre los candidatos que pasaron las columnas. Eso es lo que compra
//! PERF-11 (10M por tag+texto en <300 ms).

use mole_codec::catalog::Catalog;

use crate::pipeline::render_log;
use crate::store::{LogStore, RowKind};

#[derive(Debug, Clone, Default)]
pub enum TextFilter {
    #[default]
    None,
    /// case-sensitive por defecto; el camino rápido (Q-2 del plan)
    Substring(String),
    Regex(regex::Regex),
}

#[derive(Debug, Clone)]
pub struct LogFilter {
    /// bit i = nivel i visible (0x3F = todos) — botones segmentados UI-14
    pub levels_mask: u8,
    /// multiselección de tags; None = todos
    pub tags: Option<Vec<u16>>,
    pub task: Option<u8>,
    pub core: Option<u8>,
    pub text: TextFilter,
}

impl Default for LogFilter {
    fn default() -> Self {
        LogFilter {
            levels_mask: 0x3F,
            tags: None,
            task: None,
            core: None,
            text: TextFilter::None,
        }
    }
}

impl LogFilter {
    pub fn is_pass_through(&self) -> bool {
        self.levels_mask == 0x3F
            && self.tags.is_none()
            && self.task.is_none()
            && self.core.is_none()
            && matches!(self.text, TextFilter::None)
    }
}

/// Aplica el filtro sobre [from, to) y devuelve los índices globales que
/// pasan. Las filas especiales (UI-20) pasan siempre: son contexto que no
/// se puede ocultar.
pub fn filter_indices(
    store: &LogStore,
    catalog: &Catalog,
    filter: &LogFilter,
    from: u64,
    to: u64,
    out: &mut Vec<u64>,
) {
    out.clear();
    store.scan(from, to, |idx, row| {
        if row.kind != RowKind::Log {
            out.push(idx);
            return true;
        }
        // columnas primero: enteros, sin tocar el texto
        if row.level < 6 && (filter.levels_mask >> row.level) & 1 == 0 {
            return true;
        }
        if let Some(tags) = &filter.tags {
            if !tags.contains(&row.tag) {
                return true;
            }
        }
        if let Some(task) = filter.task {
            if row.task != task {
                return true;
            }
        }
        if let Some(core) = filter.core {
            if row.core != core {
                return true;
            }
        }
        // texto al final, solo sobre los candidatos (HOST-09)
        match &filter.text {
            TextFilter::None => out.push(idx),
            TextFilter::Substring(needle) => {
                if render_log(catalog, row.fmt_id, row.args).contains(needle.as_str()) {
                    out.push(idx);
                }
            }
            TextFilter::Regex(re) => {
                if re.is_match(&render_log(catalog, row.fmt_id, row.args)) {
                    out.push(idx);
                }
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use mole_codec::args::ArgType;
    use mole_codec::record::{FmtDef, Record};
    use mole_codec::wire::WireType;

    fn catalog_con_fmt() -> Catalog {
        let mut c = Catalog::new();
        c.apply(&Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"net".to_vec() })
            .unwrap();
        c.apply(&Record::FmtDef(FmtDef {
            fmt_id: 7,
            file_sym: 0,
            line: 1,
            arg_types: vec![ArgType::Scalar(WireType::I32)],
            fmt: b"reintento {}".to_vec(),
        }))
        .unwrap();
        c.apply(&Record::FmtDef(FmtDef {
            fmt_id: 8,
            file_sym: 0,
            line: 2,
            arg_types: vec![],
            fmt: b"listo".to_vec(),
        }))
        .unwrap();
        c
    }

    #[test]
    fn columnas_y_texto() {
        let cat = catalog_con_fmt();
        let mut store = LogStore::new(crate::store::Retention::default());
        for i in 0..100i32 {
            let level = if i % 10 == 0 { 4 } else { 2 };
            let tag = if i % 2 == 0 { 1 } else { 0 };
            store.push_log(i as u64, level, tag, 0, 0, 7, &i.to_le_bytes());
        }
        store.push_log(100, 2, 1, 0, 0, 8, &[]);

        let mut out = Vec::new();
        // solo ERROR
        let f = LogFilter { levels_mask: 1 << 4, ..Default::default() };
        filter_indices(&store, &cat, &f, 0, store.len(), &mut out);
        assert_eq!(out.len(), 10);

        // tag net + texto "reintento 4" (pasa 4 y 40..48 pares)
        let f = LogFilter {
            tags: Some(vec![1]),
            text: TextFilter::Substring("reintento 4".into()),
            ..Default::default()
        };
        filter_indices(&store, &cat, &f, 0, store.len(), &mut out);
        // pares con render que contiene "reintento 4": 4, 40,42,...,48
        assert_eq!(out, vec![4, 40, 42, 44, 46, 48]);

        // regex
        let f = LogFilter {
            text: TextFilter::Regex(regex::Regex::new(r"^listo$").unwrap()),
            ..Default::default()
        };
        filter_indices(&store, &cat, &f, 0, store.len(), &mut out);
        assert_eq!(out, vec![100]);
    }

    #[test]
    fn las_marcas_pasan_siempre() {
        let cat = catalog_con_fmt();
        let mut store = LogStore::new(crate::store::Retention::default());
        store.push_log(1, 2, 0, 0, 0, 8, &[]);
        store.push_marker(RowKind::DropMark, 2, 99);
        let f = LogFilter { levels_mask: 1 << 5, ..Default::default() }; // nada pasa
        let mut out = Vec::new();
        filter_indices(&store, &cat, &f, 0, store.len(), &mut out);
        assert_eq!(out, vec![1]); // solo la marca
    }
}
