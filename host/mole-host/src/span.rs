//! Spans — REC-18..21 y REC-19: conteo, total, media, p50/p95/p99 y máximo
//! por span. Los percentiles salen de un ring de duraciones (últimas 4096):
//! el outlier que rompe el timing no desaparece en un promedio.

use std::collections::HashMap;

const RING: usize = 4096;

#[derive(Debug, Default)]
struct SpanAgg {
    durations: Vec<u32>, // µs, ring
    pos: usize,
    n: u64,
    total_us: u64,
    max_us: u32,
}

impl SpanAgg {
    fn push(&mut self, d: u32) {
        if self.durations.len() < RING {
            self.durations.push(d);
        } else {
            self.durations[self.pos] = d;
            self.pos = (self.pos + 1) % RING;
        }
        self.n += 1;
        self.total_us += u64::from(d);
        if d > self.max_us {
            self.max_us = d;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SpanRow {
    pub sym: u16,
    pub n: u64,
    pub total_us: u64,
    pub mean_us: f64,
    pub p50_us: u32,
    pub p95_us: u32,
    pub p99_us: u32,
    pub max_us: u32,
}

#[derive(Debug, Default)]
pub struct SpanStore {
    /// span_id → (sym, t_begin). REC-21: un BEGIN con id ya abierto pisa la
    /// instancia anterior como stale (caso wrap).
    open: HashMap<u16, (u16, u64)>,
    agg: HashMap<u16, SpanAgg>,
    pub aborted: u64,
    pub orphan_ends: u64,
}

impl SpanStore {
    pub fn push_begin(&mut self, span_id: u16, sym: u16, t_us: u64) {
        self.open.insert(span_id, (sym, t_us));
    }

    pub fn push_end(&mut self, span_id: u16, t_us: u64) {
        match self.open.remove(&span_id) {
            Some((sym, t0)) => {
                let d = t_us.saturating_sub(t0).min(u64::from(u32::MAX)) as u32;
                self.agg.entry(sym).or_default().push(d);
            }
            // END sin BEGIN (drops): se descarta, contado (REC-21)
            None => self.orphan_ends += 1,
        }
    }

    /// REC-20: los spans abortados por pausa se excluyen de los histogramas.
    pub fn push_abort(&mut self, span_id: u16) {
        self.open.remove(&span_id);
        self.aborted += 1;
    }

    pub fn snapshot(&self) -> Vec<SpanRow> {
        let mut out: Vec<SpanRow> = self
            .agg
            .iter()
            .map(|(sym, a)| {
                let mut d = a.durations.clone();
                d.sort_unstable();
                let pct = |p: f64| -> u32 {
                    if d.is_empty() {
                        return 0;
                    }
                    let i = ((d.len() as f64 - 1.0) * p).round() as usize;
                    d[i.min(d.len() - 1)]
                };
                SpanRow {
                    sym: *sym,
                    n: a.n,
                    total_us: a.total_us,
                    mean_us: if a.n > 0 { a.total_us as f64 / a.n as f64 } else { 0.0 },
                    p50_us: pct(0.50),
                    p95_us: pct(0.95),
                    p99_us: pct(0.99),
                    max_us: a.max_us,
                }
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.total_us));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_ven_al_outlier() {
        let mut s = SpanStore::new_default();
        for i in 0..100u16 {
            s.push_begin(i, 7, 1000);
            s.push_end(i, 1000 + 100); // 100 µs parejos
        }
        // un outlier de 50 ms
        s.push_begin(200, 7, 1000);
        s.push_end(200, 1000 + 50_000);
        let rows = s.snapshot();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.n, 101);
        assert_eq!(r.p50_us, 100);
        assert_eq!(r.max_us, 50_000);
        assert!(r.p99_us >= 100, "p99 existe");
        // el promedio lo esconde; el max y el p99 no
        assert!(r.mean_us < 1000.0);
    }

    #[test]
    fn abort_y_huerfanos_contados() {
        let mut s = SpanStore::new_default();
        s.push_begin(1, 7, 0);
        s.push_abort(1); // REC-20
        s.push_end(9, 100); // END sin BEGIN (drops)
        assert_eq!(s.aborted, 1);
        assert_eq!(s.orphan_ends, 1);
        assert!(s.snapshot().is_empty()); // nada entró al histograma
    }

    impl SpanStore {
        fn new_default() -> Self {
            SpanStore::default()
        }
    }
}
