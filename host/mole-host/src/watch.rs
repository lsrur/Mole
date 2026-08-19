//! Watches — REC-06/REC-07: último valor, historial en ring y estadísticas
//! incrementales CORRECTAS. El promedio de v1 se rompía después de 50
//! muestras porque el contador quedaba fijado al tope del historial: acá
//! las stats son de la serie completa (Welford), no de la ventana.

use std::collections::HashMap;

pub const DEFAULT_HISTORY: usize = 10_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct WatchStats {
    pub n: u64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// desvío estándar poblacional
    pub stddev: f64,
    pub last: f64,
    pub last_t_us: u64,
}

pub struct WatchSeries {
    ring: Vec<(u64, f64)>,
    head: usize,
    filled: bool,
    // Welford sobre la serie COMPLETA
    n: u64,
    mean: f64,
    m2: f64,
    min: f64,
    max: f64,
    last: f64,
    last_t: u64,
}

impl WatchSeries {
    fn new(cap: usize) -> Self {
        WatchSeries {
            ring: Vec::with_capacity(cap),
            head: 0,
            filled: false,
            n: 0,
            mean: 0.0,
            m2: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            last: 0.0,
            last_t: 0,
        }
    }

    fn push(&mut self, t_us: u64, v: f64) {
        if self.ring.capacity() > 0 {
            if self.ring.len() < self.ring.capacity() {
                self.ring.push((t_us, v));
            } else {
                self.ring[self.head] = (t_us, v);
                self.head = (self.head + 1) % self.ring.capacity();
                self.filled = true;
            }
        }
        self.n += 1;
        let delta = v - self.mean;
        self.mean += delta / self.n as f64;
        self.m2 += delta * (v - self.mean);
        if v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
        self.last = v;
        self.last_t = t_us;
    }

    pub fn stats(&self) -> WatchStats {
        WatchStats {
            n: self.n,
            min: if self.n == 0 { 0.0 } else { self.min },
            max: if self.n == 0 { 0.0 } else { self.max },
            mean: self.mean,
            stddev: if self.n > 0 { (self.m2 / self.n as f64).sqrt() } else { 0.0 },
            last: self.last,
            last_t_us: self.last_t,
        }
    }

    /// Historial en orden cronológico (para sparkline/plot).
    pub fn history(&self) -> Vec<(u64, f64)> {
        if !self.filled {
            return self.ring.clone();
        }
        let mut out = Vec::with_capacity(self.ring.len());
        out.extend_from_slice(&self.ring[self.head..]);
        out.extend_from_slice(&self.ring[..self.head]);
        out
    }
}

#[derive(Default)]
pub struct WatchStore {
    series: HashMap<u16, WatchSeries>,
    history_cap: usize,
    dirty: Vec<u16>,
}

impl WatchStore {
    pub fn new(history_cap: usize) -> Self {
        WatchStore {
            series: HashMap::new(),
            history_cap,
            dirty: Vec::new(),
        }
    }

    pub fn push(&mut self, sym: u16, t_us: u64, v: f64) {
        let cap = self.history_cap;
        self.series
            .entry(sym)
            .or_insert_with(|| WatchSeries::new(cap))
            .push(t_us, v);
        if !self.dirty.contains(&sym) {
            self.dirty.push(sym);
        }
    }

    pub fn get(&self, sym: u16) -> Option<&WatchSeries> {
        self.series.get(&sym)
    }

    pub fn syms(&self) -> impl Iterator<Item = u16> + '_ {
        self.series.keys().copied()
    }

    /// Símbolos que cambiaron desde el último drenaje (para el Tick: solo
    /// los que cambiaron, HOST-11).
    pub fn drain_dirty(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.dirty)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn stats_correctas_mas_alla_del_historial() {
        // regresión del bug de v1: el promedio se congelaba al llenar el ring
        let mut s = WatchSeries::new(50);
        let mut sum = 0.0;
        for i in 1..=1000u32 {
            s.push(i as u64, i as f64);
            sum += i as f64;
        }
        let st = s.stats();
        assert_eq!(st.n, 1000);
        assert!((st.mean - sum / 1000.0).abs() < 1e-9, "media de la serie completa");
        assert_eq!(st.min, 1.0);
        assert_eq!(st.max, 1000.0);
        // desvío de 1..1000 uniforme discreto: sqrt((n^2-1)/12)
        let expected = ((1000.0f64 * 1000.0 - 1.0) / 12.0).sqrt();
        assert!((st.stddev - expected).abs() < 1e-6);
        // el historial retiene solo las últimas 50, en orden
        let h = s.history();
        assert_eq!(h.len(), 50);
        assert_eq!(h.first().unwrap().1, 951.0);
        assert_eq!(h.last().unwrap().1, 1000.0);
    }

    #[test]
    fn dirty_solo_los_que_cambiaron() {
        let mut ws = WatchStore::new(10);
        ws.push(5, 1, 1.5);
        ws.push(9, 2, 3.3);
        ws.push(5, 3, 1.6);
        let mut d = ws.drain_dirty();
        d.sort_unstable();
        assert_eq!(d, vec![5, 9]);
        assert!(ws.drain_dirty().is_empty());
    }
}
