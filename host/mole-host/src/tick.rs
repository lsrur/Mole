//! El contrato de IPC — HOST-10..13. Un solo evento periódico (`Tick`,
//! 30 Hz) con deltas compactos; el detalle se pide bajo demanda como
//! buffer binario (`log_slice`), nunca JSON por record.

use crate::filter::{filter_indices, LogFilter};
use crate::pipeline::{render_log, Pipeline};

/// Estado entre ticks: qué vio el último.
#[derive(Debug, Default)]
pub struct TickState {
    last_log_total: u64,
    last_records: u64,
    last_bytes_proxy: u64,
    pub seq: u64,
}

/// Arma el Tick de HOST-11. `dt_secs` es el intervalo real desde el tick
/// anterior (para las tasas).
pub fn make_tick(p: &mut Pipeline, st: &mut TickState, dt_secs: f64) -> serde_json::Value {
    st.seq += 1;
    let total = p.logs.total_ingested();
    let new_from = st.last_log_total;
    st.last_log_total = total;

    let rec_delta = p.counts.records - st.last_records;
    st.last_records = p.counts.records;
    // proxy de bytes: los frames ya des-COBSeados no guardan su largo wire;
    // se aproxima con records (el exacto llega con el contador de transporte)
    let bytes_delta = rec_delta * 16;
    st.last_bytes_proxy = bytes_delta;

    let dirty = p.watches.drain_dirty();
    let watches: Vec<serde_json::Value> = dirty
        .iter()
        .filter_map(|sym| {
            p.watches.get(*sym).map(|s| {
                let w = s.stats();
                serde_json::json!({
                    "id": sym,
                    "value": w.last,
                    "min": w.min,
                    "max": w.max,
                    "avg": w.mean,
                    "n": w.n,
                })
            })
        })
        .collect();

    serde_json::json!({
        "seq": st.seq,
        "link": {
            "recPerSec": (rec_delta as f64 / dt_secs.max(1e-6)) as u64,
            "bytesPerSec": (bytes_delta as f64 / dt_secs.max(1e-6)) as u64,
            "drops": p.counts.mcu_dropped,
            "gaps": p.counts.seq_gaps,
            "rawBytes": p.counts.raw_bytes,
        },
        "logs": {
            "total": total,
            "firstIndex": p.logs.first_index(),
            "dataSinceUs": p.logs.data_since_us(),
            "newFrom": new_from,
            "newTo": total,
        },
        "watches": watches,
        "catalogSyms": p.catalog.sym_count(),
    })
}

/// Slice binario de logs por índice GLOBAL [from, to) — HOST-12. Layout por
/// fila, little-endian:
///   u64 idx | u64 t_us | u8 kind | u8 level | u16 tag | u8 task | u8 core |
///   u16 len | len bytes de texto renderizado
pub fn log_slice_raw(p: &Pipeline, from: u64, to: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(((to - from) as usize).min(4096) * 48);
    p.logs.scan(from, to, |idx, row| {
        let text = if row.kind == crate::store::RowKind::Log {
            render_log(&p.catalog, row.fmt_id, row.args)
        } else {
            marker_text(row.kind, row.args)
        };
        push_row(&mut out, idx, row.t_us, row.kind as u8, row.level, row.tag,
                 row.task, row.core, text.as_bytes());
        true
    });
    out
}

/// Slice bajo filtro: el scroller pide posiciones DE LA LISTA FILTRADA.
/// El llamador cachea los índices con `filter_indices` y pide ventanas.
pub fn log_slice_filtered(p: &Pipeline, filter: &LogFilter, window: &[u64]) -> Vec<u8> {
    let _ = filter; // el filtro ya se aplicó al armar window; queda por firma
    let mut out = Vec::with_capacity(window.len() * 48);
    for &idx in window {
        if let Some(row) = p.logs.get(idx) {
            let text = if row.kind == crate::store::RowKind::Log {
                render_log(&p.catalog, row.fmt_id, row.args)
            } else {
                marker_text(row.kind, row.args)
            };
            push_row(&mut out, idx, row.t_us, row.kind as u8, row.level, row.tag,
                     row.task, row.core, text.as_bytes());
        }
    }
    out
}

/// Conveniencia: filtra y devuelve (total_filtrado, slice de la ventana).
pub fn query(p: &Pipeline, filter: &LogFilter, offset: usize, limit: usize)
             -> (usize, Vec<u8>) {
    let mut idx = Vec::new();
    filter_indices(&p.logs, &p.catalog, filter, p.logs.first_index(),
                   p.logs.total_ingested(), &mut idx);
    let end = (offset + limit).min(idx.len());
    let window = if offset < idx.len() { &idx[offset..end] } else { &[][..] };
    (idx.len(), log_slice_filtered(p, filter, window))
}

fn marker_text(kind: crate::store::RowKind, args: &[u8]) -> String {
    let n = if args.len() >= 4 {
        u32::from_le_bytes([args[0], args[1], args[2], args[3]])
    } else {
        0
    };
    match kind {
        crate::store::RowKind::DropMark => format!("\u{25b2} {n} records perdidos"),
        crate::store::RowKind::SessionCut => "— corte de sesión —".to_string(),
        crate::store::RowKind::PauseMark => "— pausa/reanudación —".to_string(),
        crate::store::RowKind::Log => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn push_row(out: &mut Vec<u8>, idx: u64, t: u64, kind: u8, level: u8, tag: u16,
            task: u8, core: u8, text: &[u8]) {
    out.extend_from_slice(&idx.to_le_bytes());
    out.extend_from_slice(&t.to_le_bytes());
    out.push(kind);
    out.push(level);
    out.extend_from_slice(&tag.to_le_bytes());
    out.push(task);
    out.push(core);
    let n = text.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&text[..n]);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn slice_binario_parseable() {
        let mut p = Pipeline::new(0);
        // catálogo mínimo por el camino real: un FMT_DEF y logs via wire
        use mole_codec::args::ArgType;
        use mole_codec::frame::encode_frame;
        use mole_codec::record::{FmtDef, Record};
        use mole_codec::types::TypeRegistry;
        use mole_codec::wire::WireType;
        let reg = TypeRegistry::new();
        let mut recs = vec![Record::FmtDef(FmtDef {
            fmt_id: 1,
            file_sym: 0,
            line: 1,
            arg_types: vec![ArgType::Scalar(WireType::I32)],
            fmt: b"n={}".to_vec(),
        })
        .encode(0, &reg)
        .unwrap()];
        for i in 0..5i32 {
            recs.push(
                Record::LogFmt {
                    level: 2,
                    task_id: 0,
                    core: 0,
                    tag_sym: 0,
                    fmt_id: 1,
                    args_raw: i.to_le_bytes().to_vec(),
                }
                .encode(i as u16, &reg)
                .unwrap(),
            );
        }
        let (_, wire) = encode_frame(0, 1000, 0, &recs).unwrap();
        p.feed(&wire);
        assert_eq!(p.logs.len(), 5);

        let slice = log_slice_raw(&p, 0, 5);
        // parsear la primera fila
        let idx = u64::from_le_bytes(slice[0..8].try_into().unwrap());
        let t = u64::from_le_bytes(slice[8..16].try_into().unwrap());
        assert_eq!(idx, 0);
        assert_eq!(t, 1000); // t_base + dt 0
        let len = u16::from_le_bytes(slice[22..24].try_into().unwrap()) as usize;
        assert_eq!(&slice[24..24 + len], b"n=0");

        // query con filtro pass-through
        let (total, w) = query(&p, &crate::filter::LogFilter::default(), 0, 2);
        assert_eq!(total, 5);
        assert!(!w.is_empty());
    }

    #[test]
    fn tick_reporta_deltas() {
        let mut p = Pipeline::new(0);
        let mut st = TickState::default();
        let t0 = make_tick(&mut p, &mut st, 0.033);
        assert_eq!(t0["logs"]["total"], 0);
        p.logs.push_log(1, 2, 0, 0, 0, 0, &[]);
        p.watches.push(5, 1, 1.5);
        let t1 = make_tick(&mut p, &mut st, 0.033);
        assert_eq!(t1["logs"]["newFrom"], 0);
        assert_eq!(t1["logs"]["newTo"], 1);
        assert_eq!(t1["watches"][0]["id"], 5);
        // el watch sin cambios no reaparece (HOST-11: solo los que cambiaron)
        let t2 = make_tick(&mut p, &mut st, 0.033);
        assert!(t2["watches"].as_array().unwrap().is_empty());
    }
}
