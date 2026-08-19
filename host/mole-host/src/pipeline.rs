//! Pipeline de decodificación de molectl: splitter por 0x00 (HOST-04),
//! canal raw para lo que no es Mole (HOST-05), catálogo, contabilidad de
//! seq (PR-06) y de drops del MCU (REC_STATS).

use std::collections::HashMap;

use mole_codec::catalog::Catalog;
use crate::store::{LogStore, Retention, RowKind};
use crate::watch::WatchStore;
use mole_codec::frame::{decode_wire, SeqTracker};
use mole_codec::record::Record;
use mole_codec::wire;

#[derive(Debug, Default)]
pub struct Counts {
    pub frames: u64,
    pub records: u64,
    pub by_type: HashMap<u8, u64>,
    pub crc_errors: u64,
    pub cobs_errors: u64,
    pub other_errors: u64,
    pub raw_bytes: u64,
    pub seq_gaps: u64,
    /// último REC_STATS del MCU: (enqueued, dropped)
    pub mcu_enqueued: u32,
    pub mcu_dropped: u32,
    pub counter_delta_sum: u64,
}

pub struct Pipeline {
    buf: Vec<u8>,
    pub catalog: Catalog,
    pub logs: LogStore,
    pub watches: WatchStore,
    last_dropped_seen: u32,
    seq: SeqTracker,
    pub counts: Counts,
    pub rendered: Vec<String>,
    pub render_limit: usize,
}

impl Pipeline {
    pub fn new(render_limit: usize) -> Self {
        Pipeline {
            buf: Vec::new(),
            catalog: Catalog::new(),
            logs: LogStore::new(Retention::default()),
            watches: WatchStore::new(crate::watch::DEFAULT_HISTORY),
            last_dropped_seen: 0,
            seq: SeqTracker::new(),
            counts: Counts::default(),
            rendered: Vec::new(),
            render_limit,
        }
    }

    /// Alimenta bytes crudos del transporte. Procesa todo bloque terminado
    /// en 0x00; lo que quede sin delimitador espera más datos.
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        loop {
            let Some(pos) = self.buf.iter().position(|&b| b == 0) else {
                break;
            };
            let block: Vec<u8> = self.buf.drain(..=pos).collect();
            let block = &block[..block.len() - 1]; // sin el delimitador
            if block.is_empty() {
                continue;
            }
            self.process_block(block);
        }
        // basura ilimitada sin delimitador: no crecer para siempre (SEC-04)
        if self.buf.len() > 64 * 1024 {
            self.counts.raw_bytes += self.buf.len() as u64;
            self.buf.clear();
        }
    }

    fn process_block(&mut self, block: &[u8]) {
        match decode_wire(block) {
            Ok((header, records)) => {
                self.counts.frames += 1;
                self.counts.seq_gaps += u64::from(self.seq.observe(header.seq));
                for r in &records {
                    self.counts.records += 1;
                    *self.counts.by_type.entry(r.rtype).or_insert(0) += 1;
                    let t_abs = header.t_base_us + u64::from(r.dt_us);
                    match Record::decode_payload(r.rtype, &r.payload, &self.catalog.types) {
                        Ok(rec) => self.apply(t_abs, rec),
                        Err(_) => self.counts.other_errors += 1,
                    }
                }
            }
            Err(mole_codec::DecodeError::CrcMismatch) => self.counts.crc_errors += 1,
            Err(mole_codec::DecodeError::CobsInvalid)
            | Err(mole_codec::DecodeError::CobsUnderrun) => {
                // no es Mole: canal raw (HOST-05), p. ej. el log del bootloader
                self.counts.cobs_errors += 1;
                self.counts.raw_bytes += block.len() as u64;
            }
            Err(_) => self.counts.other_errors += 1,
        }
    }

    fn apply(&mut self, t_us: u64, rec: Record) {
        let _ = self.catalog.apply(&rec);
        match &rec {
            Record::Stats(s) => {
                self.counts.mcu_enqueued = s.enqueued;
                self.counts.mcu_dropped = s.dropped;
                // UI-20/PR-14: los drops se marcan DONDE ocurrieron
                if s.dropped > self.last_dropped_seen {
                    let delta = s.dropped - self.last_dropped_seen;
                    self.logs.push_marker(RowKind::DropMark, t_us, delta);
                    self.last_dropped_seen = s.dropped;
                }
            }
            Record::Watch { sym, value } => {
                if let Some(v) = value_as_f64(&value) {
                    self.watches.push(*sym, t_us, v);
                }
            }
            Record::Counter { entries } => {
                for (_, delta) in entries {
                    self.counts.counter_delta_sum += u64::from(*delta);
                }
            }
            Record::LogFmt { fmt_id, args_raw, level, tag_sym, task_id, core } => {
                self.logs.push_log(t_us, *level, *tag_sym, *task_id, *core, *fmt_id, args_raw);
                if self.rendered.len() < self.render_limit {
                    let line = match self.catalog.parse_args(*fmt_id, args_raw) {
                        Ok(args) => {
                            let fmt = self
                                .catalog
                                .fmt(*fmt_id)
                                .map(|f| String::from_utf8_lossy(&f.fmt).into_owned())
                                .unwrap_or_else(|| "<fmt?>".into());
                            mole_fmt::format(&fmt, &args, &self.catalog)
                        }
                        Err(e) => format!("<args: {e}>"),
                    };
                    self.rendered.push(format!("[{level}] {line}"));
                }
            }
            _ => {}
        }
    }

    pub fn type_name(rtype: u8) -> &'static str {
        match rtype {
            wire::REC_SESSION => "SESSION",
            wire::REC_SYM_DEF => "SYM_DEF",
            wire::REC_STATS => "STATS",
            wire::REC_FMT_DEF => "FMT_DEF",
            wire::REC_TYPE_DEF => "TYPE_DEF",
            wire::REC_ENUM_DEF => "ENUM_DEF",
            wire::REC_LOG => "LOG",
            wire::REC_LOG_FMT => "LOG_FMT",
            wire::REC_WATCH => "WATCH",
            wire::REC_WATCH_STR => "WATCH_STR",
            wire::REC_COUNTER => "COUNTER",
            wire::REC_EVENT => "EVENT",
            wire::REC_STATE => "STATE",
            wire::REC_STATUS => "STATUS",
            _ => "?",
        }
    }

    pub fn summary_json(&self) -> serde_json::Value {
        let mut by_type: Vec<(String, u64)> = self
            .counts
            .by_type
            .iter()
            .map(|(t, n)| (format!("{} (0x{t:02x})", Self::type_name(*t)), *n))
            .collect();
        by_type.sort();
        let mut by_type_map = serde_json::Map::new();
        for (k, v) in by_type {
            by_type_map.insert(k, serde_json::json!(v));
        }
        serde_json::json!({
            "frames": self.counts.frames,
            "records": self.counts.records,
            "by_type": by_type_map,
            "seq_gaps": self.counts.seq_gaps,
            "crc_errors": self.counts.crc_errors,
            "raw_bytes": self.counts.raw_bytes,
            "decode_errors": self.counts.other_errors,
            "mcu": { "enqueued": self.counts.mcu_enqueued, "dropped": self.counts.mcu_dropped },
            "counter_delta_sum": self.counts.counter_delta_sum,
            "catalog_syms": self.catalog.sym_count(),
            "catalog_hash": format!("{:08x}", self.catalog.hash()),
        })
    }
}

/// Valor de watch como f64 (los escalares; las cadenas van por otra vista).
fn value_as_f64(v: &mole_codec::args::Value) -> Option<f64> {
    use mole_codec::args::Value as V;
    Some(match v {
        V::U8(x) => f64::from(*x),
        V::I8(x) => f64::from(*x),
        V::U16(x) => f64::from(*x),
        V::I16(x) => f64::from(*x),
        V::U32(x) => f64::from(*x),
        V::I32(x) => f64::from(*x),
        V::U64(x) => *x as f64,
        V::I64(x) => *x as f64,
        V::F32(x) => f64::from(*x),
        V::F64(x) => *x,
        V::Bool(x) => f64::from(u8::from(*x)),
        _ => return None,
    })
}

/// Renderiza una línea de log desde sus args crudos (REC-39: perezoso).
pub fn render_log(catalog: &Catalog, fmt_id: u16, args_raw: &[u8]) -> String {
    match catalog.parse_args(fmt_id, args_raw) {
        Ok(args) => {
            let fmt = catalog
                .fmt(fmt_id)
                .map(|f| String::from_utf8_lossy(&f.fmt).into_owned())
                .unwrap_or_else(|| "<fmt?>".into());
            mole_fmt::format(&fmt, &args, catalog)
        }
        Err(e) => format!("<args: {e}>"),
    }
}
