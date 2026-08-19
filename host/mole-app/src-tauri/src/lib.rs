// SPDX-License-Identifier: MIT
//! mole-app — el puente Tauri sobre mole-host (HOST-10..14).
//!
//! El contrato es el de la spec: UN evento periódico (`mole:tick`, 30 Hz)
//! con deltas compactos, y el detalle bajo demanda como buffer BINARIO
//! (`log_query`). Prohibido el patrón v1: ni un emit por record, ni JSON
//! por fila (C-2 del plan F2).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mole_host::{FileReplay, LogFilter, Pipeline, SerialTransport, TextFilter, TickState,
                Transport};
use tauri::{Emitter, State};

struct Shared {
    pipeline: Mutex<Pipeline>,
    tick: Mutex<TickState>,
    /// stop-flag del lector vigente; conectar de nuevo mata al anterior
    reader_stop: Mutex<Arc<AtomicBool>>,
    source_desc: Mutex<String>,
}

impl Shared {
    fn new() -> Self {
        Shared {
            pipeline: Mutex::new(Pipeline::new(0)),
            tick: Mutex::new(TickState::default()),
            reader_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            source_desc: Mutex::new(String::new()),
        }
    }
}

type AppState<'a> = State<'a, Arc<Shared>>;

fn spawn_reader(shared: &Arc<Shared>, mut transport: Box<dyn Transport>) {
    let my_stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = shared.reader_stop.lock() {
        slot.store(true, Ordering::SeqCst); // matar al lector anterior
        *slot = my_stop.clone();
    }
    let shared = shared.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            if my_stop.load(Ordering::Relaxed) {
                return;
            }
            match transport.read_some(&mut buf) {
                Ok(0) => {
                    if transport.finished() {
                        return; // replay agotado: el store queda servible
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(n) => {
                    if let Ok(mut p) = shared.pipeline.lock() {
                        p.feed(&buf[..n]);
                    }
                }
                // el puerto desapareció (reset del MCU es flujo normal,
                // TR-09); la reconexión con backoff llega en B-02
                Err(_) => return,
            }
        }
    });
}

#[tauri::command]
fn open_replay(state: AppState, path: String) -> Result<String, String> {
    let replay = FileReplay::open(&path).map_err(|e| e.to_string())?;
    if let Ok(mut d) = state.source_desc.lock() {
        *d = format!("replay: {path}");
    }
    spawn_reader(state.inner(), Box::new(replay));
    Ok(format!("replay abierto: {path}"))
}

#[tauri::command]
fn connect_serial(state: AppState, port: String, baud: u32) -> Result<String, String> {
    // la primera apertura valida el puerto; después el lector se reconecta
    // solo con backoff 100 ms → 5 s (TR-09: el reset del MCU hace
    // desaparecer y reaparecer el device — es el flujo normal, no un error)
    let t = SerialTransport::open(&port, baud).map_err(|e| e.to_string())?;
    if let Ok(mut d) = state.source_desc.lock() {
        *d = format!("serial: {port} @{baud}");
    }
    let shared = state.inner().clone();
    let my_stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = shared.reader_stop.lock() {
        slot.store(true, Ordering::SeqCst);
        *slot = my_stop.clone();
    }
    let port_owned = port.clone();
    std::thread::spawn(move || {
        let port = port_owned;
        let mut transport: Option<Box<dyn Transport>> = Some(Box::new(t));
        let mut backoff_ms = 100u64;
        let mut buf = [0u8; 8192];
        loop {
            if my_stop.load(Ordering::Relaxed) {
                return;
            }
            match transport.as_mut() {
                Some(tr) => match tr.read_some(&mut buf) {
                    Ok(0) => std::thread::sleep(Duration::from_millis(2)),
                    Ok(n) => {
                        backoff_ms = 100;
                        if let Ok(mut p) = shared.pipeline.lock() {
                            p.feed(&buf[..n]);
                        }
                    }
                    Err(_) => transport = None, // el puerto desapareció
                },
                None => {
                    std::thread::sleep(Duration::from_millis(backoff_ms));
                    backoff_ms = (backoff_ms * 2).min(5000);
                    if let Ok(t) = SerialTransport::open(&port, baud) {
                        transport = Some(Box::new(t));
                    }
                }
            }
        }
    });
    Ok(format!("conectado a {port}"))
}

#[tauri::command]
fn list_ports() -> Vec<serde_json::Value> {
    mole_host::list_ports()
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FilterSpec {
    levels_mask: Option<u8>,
    tags: Option<Vec<u16>>,
    task: Option<u8>,
    core: Option<u8>,
    text: Option<String>,
    regex: Option<bool>,
}

fn to_filter(spec: &FilterSpec) -> Result<LogFilter, String> {
    let mut f = LogFilter {
        levels_mask: spec.levels_mask.unwrap_or(0x3F),
        tags: spec.tags.clone(),
        task: spec.task,
        core: spec.core,
        ..Default::default()
    };
    if let Some(text) = &spec.text {
        if !text.is_empty() {
            f.text = if spec.regex.unwrap_or(false) {
                TextFilter::Regex(
                    mole_host::Regex::new(text).map_err(|e| e.to_string())?,
                )
            } else {
                TextFilter::Substring(text.clone())
            };
        }
    }
    Ok(f)
}

/// HOST-12: la ventana visible del scroller, como buffer binario.
/// Layout: u64 total_filtrado | filas (ver mole_host::tick).
#[tauri::command]
fn log_query(
    state: AppState,
    filter: FilterSpec,
    offset: usize,
    limit: usize,
) -> Result<tauri::ipc::Response, String> {
    let f = to_filter(&filter)?;
    let p = state.pipeline.lock().map_err(|e| e.to_string())?;
    let (total, slice) = mole_host::query(&p, &f, offset, limit.min(2000));
    let mut out = Vec::with_capacity(8 + slice.len());
    out.extend_from_slice(&(total as u64).to_le_bytes());
    out.extend_from_slice(&slice);
    Ok(tauri::ipc::Response::new(out))
}

/// Snapshot completo de watches (UI-27). Chico: decenas de filas.
#[tauri::command]
fn watch_snapshot(state: AppState) -> Result<Vec<serde_json::Value>, String> {
    let p = state.pipeline.lock().map_err(|e| e.to_string())?;
    let mut out: Vec<serde_json::Value> = p
        .watches
        .syms()
        .filter_map(|sym| {
            p.watches.get(sym).map(|s| {
                let w = s.stats();
                let name = p
                    .catalog
                    .sym(sym)
                    .map(|e| String::from_utf8_lossy(&e.name).into_owned())
                    .unwrap_or_else(|| format!("#{sym}"));
                serde_json::json!({
                    "id": sym,
                    "name": name,
                    "last": w.last,
                    "min": w.min,
                    "max": w.max,
                    "mean": w.mean,
                    "stddev": w.stddev,
                    "n": w.n,
                    "history": s.history().iter().rev().take(60).rev()
                        .map(|(_, v)| v).collect::<Vec<_>>(),
                })
            })
        })
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// Nombres y kinds de símbolos: `{id: [nombre, kind]}`. El kind permite a
/// la UI poblar la multiselección de tags (CAT-03: Tag=2).
#[tauri::command]
fn sym_names(state: AppState) -> Result<serde_json::Value, String> {
    let p = state.pipeline.lock().map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    for id in 1..=u16::MAX {
        match p.catalog.sym(id) {
            Some(s) => {
                map.insert(
                    id.to_string(),
                    serde_json::json!([String::from_utf8_lossy(&s.name), s.kind]),
                );
            }
            None => break, // ids secuenciales (CAT-01): el primero ausente corta
        }
    }
    Ok(serde_json::Value::Object(map))
}

#[tauri::command]
fn source_desc(state: AppState) -> String {
    state
        .source_desc
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

pub fn run() {
    let shared = Arc::new(Shared::new());
    let shared_for_tick = shared.clone();
    tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            open_replay,
            connect_serial,
            list_ports,
            log_query,
            watch_snapshot,
            sym_names,
            source_desc
        ])
        .setup(move |app| {
            // HOST-11: el único evento periódico, 30 Hz
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(33));
                    let dt = last.elapsed().as_secs_f64();
                    last = Instant::now();
                    let tick = {
                        let Ok(mut p) = shared_for_tick.pipeline.lock() else {
                            continue;
                        };
                        let Ok(mut ts) = shared_for_tick.tick.lock() else {
                            continue;
                        };
                        mole_host::make_tick(&mut p, &mut ts, dt)
                    };
                    let _ = handle.emit("mole:tick", tick);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar mole-app");
}
