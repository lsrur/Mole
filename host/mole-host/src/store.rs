//! Store columnar de logs — HOST-06..09, ARQ-02. Append-only, paginado
//! para poder descartar por retención (PA-06: descarte con aviso).
//!
//! Cada fila es columnas de enteros + los argumentos CRUDOS tipados
//! (REC-39: el texto se renderiza perezosamente; reformateo retroactivo y
//! filtro por valor siguen siendo posibles).

use std::collections::VecDeque;

/// Filas especiales dentro del stream (UI-20), no en un contador aparte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Log = 0,
    /// "▲ N records perdidos" — payload = N
    DropMark = 1,
    /// cambió el firmware o hubo reset (DX-02)
    SessionCut = 2,
    /// el MCU entró/salió de un checkpoint
    PauseMark = 3,
}

#[derive(Debug, Clone)]
pub struct LogRow<'a> {
    pub kind: RowKind,
    pub t_us: u64,
    pub level: u8,
    pub tag: u16,
    pub task: u8,
    pub core: u8,
    pub fmt_id: u16,
    /// para Log: args crudos; para marcas: payload auxiliar
    pub args: &'a [u8],
}

struct Page {
    kind: Vec<u8>,
    t_us: Vec<u64>,
    level: Vec<u8>,
    tag: Vec<u16>,
    task: Vec<u8>,
    core: Vec<u8>,
    fmt_id: Vec<u16>,
    arg_off: Vec<u32>,
    arg_len: Vec<u16>,
    args: Vec<u8>,
}

const PAGE_ARG_BYTES: usize = 1 << 20; // 1 MB de arena de args (HOST-07)
const PAGE_MAX_ROWS: usize = 64 * 1024;

impl Page {
    fn new() -> Self {
        Page {
            kind: Vec::new(),
            t_us: Vec::new(),
            level: Vec::new(),
            tag: Vec::new(),
            task: Vec::new(),
            core: Vec::new(),
            fmt_id: Vec::new(),
            arg_off: Vec::new(),
            arg_len: Vec::new(),
            args: Vec::with_capacity(64 * 1024),
        }
    }

    fn len(&self) -> usize {
        self.kind.len()
    }

    fn full(&self) -> bool {
        self.len() >= PAGE_MAX_ROWS || self.args.len() >= PAGE_ARG_BYTES
    }

    fn bytes(&self) -> usize {
        self.len() * (1 + 8 + 1 + 2 + 1 + 1 + 2 + 4 + 2) + self.args.len()
    }

    fn row(&self, i: usize) -> LogRow<'_> {
        let off = self.arg_off[i] as usize;
        let len = self.arg_len[i] as usize;
        LogRow {
            kind: match self.kind[i] {
                1 => RowKind::DropMark,
                2 => RowKind::SessionCut,
                3 => RowKind::PauseMark,
                _ => RowKind::Log,
            },
            t_us: self.t_us[i],
            level: self.level[i],
            tag: self.tag[i],
            task: self.task[i],
            core: self.core[i],
            fmt_id: self.fmt_id[i],
            args: &self.args[off..off + len],
        }
    }
}

/// Límites de retención (HOST-08). Al superar cualquiera se descarta la
/// página más vieja, y `first_index()`/`data_since_us()` avisan (PA-06).
#[derive(Debug, Clone, Copy)]
pub struct Retention {
    pub max_records: u64,
    pub max_bytes: usize,
}

impl Default for Retention {
    fn default() -> Self {
        Retention { max_records: 10_000_000, max_bytes: 1 << 30 }
    }
}

pub struct LogStore {
    pages: VecDeque<Page>,
    retention: Retention,
    first_index: u64,
    total: u64,
    bytes: usize,
    discarded: u64,
}

impl LogStore {
    pub fn new(retention: Retention) -> Self {
        LogStore {
            pages: VecDeque::new(),
            retention,
            first_index: 0,
            total: 0,
            bytes: 0,
            discarded: 0,
        }
    }

    pub fn push_log(&mut self, t_us: u64, level: u8, tag: u16, task: u8, core: u8,
                    fmt_id: u16, args: &[u8]) {
        self.push_row(RowKind::Log, t_us, level, tag, task, core, fmt_id, args);
    }

    /// Fila especial en el stream (UI-20): marca de drops, corte, pausa.
    pub fn push_marker(&mut self, kind: RowKind, t_us: u64, payload: u32) {
        self.push_row(kind, t_us, 0, 0, 0, 0, 0, &payload.to_le_bytes());
    }

    #[allow(clippy::too_many_arguments)]
    fn push_row(&mut self, kind: RowKind, t_us: u64, level: u8, tag: u16, task: u8,
                core: u8, fmt_id: u16, args: &[u8]) {
        if self.pages.back().map(Page::full).unwrap_or(true) {
            self.pages.push_back(Page::new());
        }
        // `unwrap` imposible: recién garantizamos una página
        let Some(page) = self.pages.back_mut() else { return };
        let prev = page.bytes();
        page.kind.push(kind as u8);
        page.t_us.push(t_us);
        page.level.push(level);
        page.tag.push(tag);
        page.task.push(task);
        page.core.push(core);
        page.fmt_id.push(fmt_id);
        page.arg_off.push(page.args.len() as u32);
        page.arg_len.push(args.len().min(u16::MAX as usize) as u16);
        page.args.extend_from_slice(args);
        self.bytes += page.bytes() - prev;
        self.total += 1;
        self.enforce_retention();
    }

    fn enforce_retention(&mut self) {
        while self.len() > self.retention.max_records
            || self.bytes > self.retention.max_bytes
        {
            let Some(old) = self.pages.pop_front() else { break };
            self.first_index += old.len() as u64;
            self.discarded += old.len() as u64;
            self.bytes -= old.bytes();
        }
    }

    /// Cantidad de filas retenidas.
    pub fn len(&self) -> u64 {
        self.total - self.first_index
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Índice global de la primera fila disponible (>0 ⇒ hubo descarte).
    pub fn first_index(&self) -> u64 {
        self.first_index
    }

    /// Total ingerido en la vida de la sesión.
    pub fn total_ingested(&self) -> u64 {
        self.total
    }

    pub fn discarded(&self) -> u64 {
        self.discarded
    }

    /// Timestamp de la fila más vieja retenida — el aviso "datos desde".
    pub fn data_since_us(&self) -> Option<u64> {
        self.pages.front().and_then(|p| p.t_us.first().copied())
    }

    pub fn approx_bytes(&self) -> usize {
        self.bytes
    }

    /// Fila por índice GLOBAL (estable frente al descarte).
    pub fn get(&self, index: u64) -> Option<LogRow<'_>> {
        if index < self.first_index {
            return None;
        }
        let mut rel = (index - self.first_index) as usize;
        for page in &self.pages {
            if rel < page.len() {
                return Some(page.row(rel));
            }
            rel -= page.len();
        }
        None
    }

    /// Recorre [from, to) global llamando f; corta si f devuelve false.
    pub fn scan<F: FnMut(u64, LogRow<'_>) -> bool>(&self, from: u64, to: u64, mut f: F) {
        let from = from.max(self.first_index);
        if from >= to {
            return;
        }
        let mut idx = self.first_index;
        for page in &self.pages {
            let page_end = idx + page.len() as u64;
            if page_end <= from {
                idx = page_end;
                continue;
            }
            let start = from.saturating_sub(idx) as usize;
            for i in start..page.len() {
                let global = idx + i as u64;
                if global >= to {
                    return;
                }
                if !f(global, page.row(i)) {
                    return;
                }
            }
            idx = page_end;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn indices_globales_estables_bajo_descarte() {
        // páginas chicas via retención por records
        let mut s = LogStore::new(Retention { max_records: 150_000, max_bytes: usize::MAX });
        for i in 0..300_000u64 {
            s.push_log(i, 2, 1, 0, 0, 7, &i.to_le_bytes());
        }
        assert!(s.first_index() > 0, "hubo descarte");
        assert_eq!(s.total_ingested(), 300_000);
        // toda fila retenida conserva su índice global
        let first = s.first_index();
        let row = s.get(first).unwrap();
        assert_eq!(row.t_us, first); // t == índice por construcción
        assert!(s.get(first - 1).is_none(), "lo descartado no existe");
        assert_eq!(s.data_since_us(), Some(first));
        // scan respeta límites
        let mut seen = 0u64;
        s.scan(first + 10, first + 20, |g, r| {
            assert_eq!(r.t_us, g);
            seen += 1;
            true
        });
        assert_eq!(seen, 10);
    }

    #[test]
    fn marcas_especiales_en_el_stream() {
        let mut s = LogStore::new(Retention::default());
        s.push_log(100, 2, 1, 0, 0, 7, &[]);
        s.push_marker(RowKind::DropMark, 150, 1204);
        s.push_log(200, 3, 1, 0, 0, 7, &[]);
        let m = s.get(1).unwrap();
        assert_eq!(m.kind, RowKind::DropMark);
        assert_eq!(u32::from_le_bytes(m.args.try_into().unwrap()), 1204);
    }
}
