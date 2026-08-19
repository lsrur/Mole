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
    /// cola de frames de downlink hacia el lector serial (PR-15)
    downlink_tx: Mutex<Option<std::sync::mpsc::Sender<Vec<u8>>>>,
    source_desc: Mutex<String>,
}

impl Shared {
    fn new() -> Self {
        Shared {
            pipeline: Mutex::new(Pipeline::new(0)),
            tick: Mutex::new(TickState::default()),
            reader_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            downlink_tx: Mutex::new(None),
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
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    if let Ok(mut slot) = shared.downlink_tx.lock() {
        *slot = Some(tx);
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
                Some(tr) => {
                    // downlink pendiente (CTL_SET_LEVEL, etc.)
                    while let Ok(frame) = rx.try_recv() {
                        let _ = tr.write_all_frame(&frame);
                    }
                    match tr.read_some(&mut buf) {
                        Ok(0) => std::thread::sleep(Duration::from_millis(2)),
                        Ok(n) => {
                            backoff_ms = 100;
                            if let Ok(mut p) = shared.pipeline.lock() {
                                p.feed(&buf[..n]);
                            }
                        }
                        Err(_) => transport = None, // el puerto desapareció
                    }
                }
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

/// FEAT-03: nivel de log por tag, en vivo, sin recompilar (PR-19). El
/// firmware ya lo honra (F1-T09); acá solo viaja el CTL_SET_LEVEL.
#[tauri::command]
fn set_tag_level(state: AppState, sym: u16, level: u8) -> Result<(), String> {
    use mole_codec::frame::encode_frame;
    use mole_codec::record::Record;
    use mole_codec::types::TypeRegistry;
    let reg = TypeRegistry::new();
    let rec = Record::CtlSetLevel { sym_id: sym, level }
        .encode(0, &reg)
        .map_err(|e| e.to_string())?;
    let (_, wire) = encode_frame(0, 0, 0, &[rec]).map_err(|e| e.to_string())?;
    let guard = state.downlink_tx.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(tx) => tx.send(wire).map_err(|e| e.to_string()),
        None => Err("sin transporte con downlink (replay o demo)".into()),
    }
}

/// F2-B09: fuente sintética para medir PERF-09 (UI a 60 fps con 50k rec/s).
/// Genera frames reales por el pipeline completo, sin hardware.
#[tauri::command]
fn demo_start(state: AppState, rate: u32) -> Result<String, String> {
    use mole_codec::args::ArgType;
    use mole_codec::frame::encode_frame;
    use mole_codec::record::{FmtDef, Record};
    use mole_codec::types::TypeRegistry;
    use mole_codec::wire::WireType;

    let shared = state.inner().clone();
    let my_stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = shared.reader_stop.lock() {
        slot.store(true, Ordering::SeqCst);
        *slot = my_stop.clone();
    }
    if let Ok(mut d) = state.source_desc.lock() {
        *d = format!("demo: {rate} rec/s sintéticos");
    }
    std::thread::spawn(move || {
        let reg = TypeRegistry::new();
        // catálogo del demo: tag + fmt, una sola vez
        let defs = vec![
            Record::SymDef { sym_id: 1, kind: 2, parent: 0, name: b"demo".to_vec() }
                .encode(0, &reg)
                .unwrap_or_default(),
            Record::SymDef { sym_id: 2, kind: 1, parent: 0, name: b"seno".to_vec() }
                .encode(0, &reg)
                .unwrap_or_default(),
            Record::FmtDef(FmtDef {
                fmt_id: 1,
                file_sym: 0,
                line: 1,
                arg_types: vec![ArgType::Scalar(WireType::I32), ArgType::Scalar(WireType::F32)],
                fmt: b"iter={} v={:.2}".to_vec(),
            })
            .encode(0, &reg)
            .unwrap_or_default(),
        ];
        let mut seq: u16 = 0;
        let mut t_us: u64 = 1_000_000;
        let mut i: u32 = 0;
        let mut span_id: u16 = 1;
        // spans y comandos del demo: syms 3/4 (spans), 5/6 (comandos)
        let mut defs = defs;
        for (id, kind, name) in [
            (3u16, 6u8, &b"tx_cycle"[..]),
            (4, 6, b"build_frame"),
            (5, 5, b"Reset radio"),
            (6, 5, b"Set channel"),
        ] {
            defs.push(
                Record::SymDef { sym_id: id, kind, parent: 0, name: name.to_vec() }
                    .encode(0, &reg)
                    .unwrap_or_default(),
            );
        }
        defs.push(
            Record::CmdDef { cmd_id: 1, sym: 5, arg_type: 0, min: None, max: None }
                .encode(0, &reg)
                .unwrap_or_default(),
        );
        defs.push(
            Record::CmdDef {
                cmd_id: 2,
                sym: 6,
                arg_type: 0x06,
                min: Some(mole_codec::args::Value::I32(0)),
                max: Some(mole_codec::args::Value::I32(15)),
            }
            .encode(0, &reg)
            .unwrap_or_default(),
        );
        if let Ok((_, wire)) = encode_frame(seq, t_us, 0, &defs) {
            seq = seq.wrapping_add(1);
            if let Ok(mut p) = shared.pipeline.lock() {
                p.feed(&wire);
            }
        }
        loop {
            if my_stop.load(Ordering::Relaxed) {
                return;
            }
            // tanda de 20 ms
            let n = (rate / 50).max(1);
            let mut recs = Vec::with_capacity(n as usize);
            for k in 0..n {
                let dt = (k * 20_000 / n.max(1)) as u16;
                let rec = if k % 2 == 0 {
                    let v = ((i as f32) * 0.01).sin() * 2.5;
                    let mut args = Vec::with_capacity(8);
                    args.extend_from_slice(&(i as i32).to_le_bytes());
                    args.extend_from_slice(&v.to_le_bytes());
                    Record::LogFmt {
                        level: (i % 6) as u8,
                        task_id: 1,
                        core: 0,
                        tag_sym: 1,
                        fmt_id: 1,
                        args_raw: args,
                    }
                } else {
                    Record::Watch {
                        sym: 2,
                        value: mole_codec::args::Value::F32(((i as f32) * 0.02).sin()),
                    }
                };
                if let Ok(bytes) = rec.encode(dt, &reg) {
                    recs.push(bytes);
                }
                i = i.wrapping_add(1);
            }
            // un árbol de spans por tanda: tx_cycle conteniendo build_frame,
            // con jitter y un outlier cada ~200 iteraciones (para el p99)
            {
                let tx = span_id;
                span_id = span_id.wrapping_add(1).max(1);
                let build = span_id;
                span_id = span_id.wrapping_add(1).max(1);
                let jitter = u64::from(i % 37) * 20;
                let outlier = if i % 4000 < 20 { 9000 } else { 0 };
                let mut p = |rec: Record, dt: u16| {
                    if let Ok(b) = rec.encode(dt, &reg) {
                        recs.push(b);
                    }
                };
                p(Record::SpanBegin { span_id: tx, sym: 3, parent_span_id: 0, task_id: 1 }, 0);
                p(Record::SpanBegin { span_id: build, sym: 4, parent_span_id: tx, task_id: 1 }, 100);
                p(Record::SpanEnd { span_id: build }, (600 + jitter) as u16);
                p(Record::SpanEnd { span_id: tx }, (2500 + jitter + outlier) as u16);
            }
            // en frames de a 200 (PR-04 los limita por tamaño igual)
            for chunk in recs.chunks(200) {
                if let Ok((_, wire)) = encode_frame(seq, t_us, 0, chunk) {
                    seq = seq.wrapping_add(1);
                    if let Ok(mut p) = shared.pipeline.lock() {
                        p.feed(&wire);
                    }
                }
            }
            t_us += 20_000;
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    Ok(format!("demo a {rate} rec/s"))
}

/// Tabla de spans (UI-31): n, total, media, p50/p95/p99, max (REC-19).
#[tauri::command]
fn span_snapshot(state: AppState) -> Result<Vec<serde_json::Value>, String> {
    let p = state.pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.spans
        .snapshot()
        .into_iter()
        .map(|r| {
            let name = p
                .catalog
                .sym(r.sym)
                .map(|s| String::from_utf8_lossy(&s.name).into_owned())
                .unwrap_or_else(|| format!("#{}", r.sym));
            let mut v = serde_json::to_value(&r).unwrap_or_default();
            v["name"] = serde_json::json!(name);
            v
        })
        .collect())
}

/// Comandos declarados por el firmware (REC-30): la UI se genera de acá.
#[tauri::command]
fn command_list(state: AppState) -> Result<Vec<serde_json::Value>, String> {
    let p = state.pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.commands
        .iter()
        .map(|c| {
            let name = p
                .catalog
                .sym(c.sym)
                .map(|s| String::from_utf8_lossy(&s.name).into_owned())
                .unwrap_or_else(|| format!("#{}", c.sym));
            let mut v = serde_json::to_value(c).unwrap_or_default();
            v["name"] = serde_json::json!(name);
            v
        })
        .collect())
}

/// FEAT-27: invoca un comando del firmware (CTL_CMD por downlink).
#[tauri::command]
fn send_command(
    state: AppState,
    cmd_id: u16,
    arg_type: u8,
    value: Option<f64>,
) -> Result<(), String> {
    use mole_codec::args::Value;
    use mole_codec::frame::encode_frame;
    use mole_codec::record::Record;
    use mole_codec::types::TypeRegistry;
    let arg = match (arg_type, value) {
        (0, _) => None,
        (0x06, Some(v)) => Some(Value::I32(v as i32)),
        (0x09, Some(v)) => Some(Value::F32(v as f32)),
        _ => return Err("argumento invalido para el tipo del comando".into()),
    };
    let reg = TypeRegistry::new();
    let rec = Record::CtlCmd { cmd_id, arg_type, arg }
        .encode(0, &reg)
        .map_err(|e| e.to_string())?;
    let (_, wire) = encode_frame(0, 0, 0, &[rec]).map_err(|e| e.to_string())?;
    let guard = state.downlink_tx.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(tx) => tx.send(wire).map_err(|e| e.to_string()),
        None => Err("sin transporte con downlink (replay o demo)".into()),
    }
}

/// FEAT-07 / UI-07: desprender un panel a ventana propia. Veredicto del
/// spike UI-08: el popout de dockview NO anda bajo Tauri (WKWebView bloquea
/// window.open), así que la ventana desprendida es una ventana Tauri real
/// que consume el mismo contrato de IPC (HOST-14) — a diferencia de v1,
/// que mostraba datos mock.
#[tauri::command]
fn detach_panel(app: tauri::AppHandle, panel: String) -> Result<(), String> {
    if !matches!(panel.as_str(), "watch" | "log") {
        return Err(format!("panel desconocido: {panel}"));
    }
    let label = format!("panel-{panel}");
    if let Some(w) = tauri::Manager::get_webview_window(&app, &label) {
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(format!("index.html?panel={panel}").into()),
    )
    .title(format!("Mole — {panel}"))
    .inner_size(560.0, 640.0)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
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
            set_tag_level,
            demo_start,
            detach_panel,
            span_snapshot,
            command_list,
            send_command,
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
