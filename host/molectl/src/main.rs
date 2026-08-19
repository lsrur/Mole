//! molectl — el pipeline completo sin UI (ARQ-03, FEAT-48).
//!
//! Subcomandos de F1:
//!   decode-file <ruta> [--render N]   decodifica un stream crudo (p. ej. el
//!                                     e2e_stream.bin del test de C++)
//!   watch <puerto> [--baud N]         abre el puerto y muestra salud en vivo
//!   ports                             lista puertos, marca los conocidos
//!   bench-decoder [--frames N]        HOST-03: records/s del decoder en un core
//!   bench                             (T-12: necesita la placa)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use mole_host::Pipeline;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn cmd_decode_file(path: &str, render: usize) -> ExitCode {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("no pude leer {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut p = Pipeline::new(render);
    // alimentar en trozos irregulares: ejercita el splitter incremental
    for chunk in data.chunks(4093) {
        p.feed(chunk);
    }
    println!("{}", serde_json::to_string_pretty(&p.summary_json()).unwrap());
    for line in &p.rendered {
        println!("{line}");
    }
    let ok = p.counts.crc_errors == 0
        && p.counts.other_errors == 0
        && p.counts.seq_gaps == 0
        && p.counts.frames > 0;
    if ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("stream con errores o huecos");
        ExitCode::FAILURE
    }
}

/// VID conocidos: Espressif nativo y puentes USB-serial habituales (DX-03).
fn known_vid(vid: u16) -> Option<&'static str> {
    match vid {
        0x303A => Some("Espressif (USB nativo)"),
        0x10C4 => Some("Silicon Labs CP210x"),
        0x1A86 => Some("WCH CH340"),
        0x0403 => Some("FTDI"),
        _ => None,
    }
}

fn cmd_ports() -> ExitCode {
    match serialport::available_ports() {
        Ok(ports) if !ports.is_empty() => {
            for p in ports {
                let extra = match &p.port_type {
                    serialport::SerialPortType::UsbPort(u) => {
                        let tag = known_vid(u.vid).unwrap_or("desconocido");
                        format!("usb {:04x}:{:04x} {tag}", u.vid, u.pid)
                    }
                    other => format!("{other:?}"),
                };
                println!("{}  {extra}", p.port_name);
            }
            ExitCode::SUCCESS
        }
        Ok(_) => {
            println!("sin puertos serie");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_watch(port_name: &str, baud: u32, render: usize) -> ExitCode {
    let port = serialport::new(port_name, baud)
        .timeout(std::time::Duration::from_millis(100))
        .open();
    let mut port = match port {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no pude abrir {port_name}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut p = Pipeline::new(render);
    let mut buf = [0u8; 4096];
    let mut last_print = Instant::now();
    let mut printed_logs = 0usize;
    loop {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => p.feed(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("lectura: {e}");
                return ExitCode::FAILURE;
            }
        }
        while printed_logs < p.rendered.len() {
            println!("{}", p.rendered[printed_logs]);
            printed_logs += 1;
        }
        if last_print.elapsed().as_secs() >= 1 {
            last_print = Instant::now();
            eprintln!(
                "-- frames {}  rec {}  gaps {}  crc {}  raw {}B  drops MCU {}",
                p.counts.frames,
                p.counts.records,
                p.counts.seq_gaps,
                p.counts.crc_errors,
                p.counts.raw_bytes,
                p.counts.mcu_dropped
            );
        }
    }
}

/// HOST-03: el decoder tiene que sostener >=500k records/s en un core.
fn cmd_bench_decoder(frames: usize) -> ExitCode {
    use mole_codec::args::Value;
    use mole_codec::frame::encode_frame;
    use mole_codec::record::Record;
    use mole_codec::types::TypeRegistry;

    // stream sintético representativo: logs de dos escalares (REC-04)
    let reg = TypeRegistry::new();
    let rec = Record::LogFmt {
        level: 2,
        task_id: 3,
        core: 1,
        tag_sym: 1,
        fmt_id: 7,
        args_raw: vec![0x00, 0x04, 0x00, 0x00, 0x7b, 0x14, 0x1e, 0x40],
    }
    .encode(1500, &reg)
    .unwrap();
    let watch = Record::Watch { sym: 5, value: Value::F32(1.5) }
        .encode(10, &reg)
        .unwrap();
    let mut per_frame = Vec::new();
    for i in 0..200 {
        per_frame.push(if i % 2 == 0 { rec.clone() } else { watch.clone() });
    }
    let mut stream = Vec::new();
    for seq in 0..frames {
        let (_, wire) = encode_frame(seq as u16, seq as u64 * 1000, 0, &per_frame).unwrap();
        stream.extend_from_slice(&wire);
    }
    let total_records = (frames * per_frame.len()) as u64;

    let mut p = Pipeline::new(0);
    let t0 = Instant::now();
    p.feed(&stream);
    let dt = t0.elapsed();
    let rate = total_records as f64 / dt.as_secs_f64();
    println!(
        "{}",
        serde_json::json!({
            "records": total_records,
            "bytes": stream.len(),
            "seconds": dt.as_secs_f64(),
            "records_per_sec": rate as u64,
            "objetivo_host03": 500000,
            "cumple": rate >= 500_000.0,
        })
    );
    if p.counts.records != total_records || p.counts.crc_errors != 0 {
        eprintln!("el decoder perdió records en el benchmark");
        return ExitCode::FAILURE;
    }
    if rate >= 500_000.0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// F1-T12 (TEST-05): corre el banco. El firmware de examples/bench se
/// auto-rampa y reporta por el propio enlace; acá se mide, se verifica la
/// contabilidad (TEST-04) y se compara contra el baseline commiteado.
fn cmd_bench(port_name: &str, baud: u32, baseline: Option<String>) -> ExitCode {
    use mole_codec::frame::encode_frame;
    use mole_codec::record::Record;
    use mole_codec::types::TypeRegistry;

    let port = serialport::new(port_name, baud)
        .timeout(std::time::Duration::from_millis(50))
        .open();
    let mut port = match port {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no pude abrir {port_name}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // CAT-09: HELLO de un host que no sabe nada → resync + REC_SESSION
    let reg = TypeRegistry::new();
    let hello = Record::CtlHello { proto_ver: 2, known_epoch: 0, known_catalog_hash: 0 }
        .encode(0, &reg)
        .unwrap();
    let (_, wire) = encode_frame(0, 0, 0, &[hello]).unwrap();
    let _ = std::io::Write::write_all(&mut port, &wire);

    let mut p = Pipeline::new(usize::MAX);
    let mut buf = [0u8; 4096];
    let mut seen_lines = 0usize;
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut costs: Vec<String> = Vec::new();
    let mut burst: Option<String> = None;
    let mut max_clean_rate = 0u64;
    let mut prev_drop = 0u32;
    let started = Instant::now();

    loop {
        match std::io::Read::read(&mut port, &mut buf) {
            Ok(n) if n > 0 => p.feed(&buf[..n]),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("lectura: {e}");
                return ExitCode::FAILURE;
            }
        }
        let mut done = false;
        while seen_lines < p.rendered.len() {
            let line = p.rendered[seen_lines].clone();
            seen_lines += 1;
            println!("{line}");
            if line.contains("cost ") {
                costs.push(line.clone());
            }
            if line.contains("burst done") {
                burst = Some(line.clone());
            }
            if line.contains("step done") {
                // "step done rate={} logs={} watches={} enq={} drop={} heap={}"
                let num = |key: &str| -> u64 {
                    line.split(&format!("{key}="))
                        .nth(1)
                        .and_then(|s| {
                            s.split_whitespace().next().and_then(|v| v.parse().ok())
                        })
                        .unwrap_or(0)
                };
                let rate = num("rate");
                let drop_now = p.counts.mcu_dropped;
                let step_drops = drop_now.saturating_sub(prev_drop);
                prev_drop = drop_now;
                if step_drops == 0 && rate > max_clean_rate {
                    max_clean_rate = rate;  // PERF-06: sostenido sin pérdida
                }
                steps.push(serde_json::json!({
                    "rate": rate,
                    "attempted": num("logs") + num("watches"),
                    "drops_en_el_paso": step_drops,
                }));
            }
            if line.contains("bench fin") {
                done = true;
            }
        }
        if done {
            break;
        }
        if started.elapsed().as_secs() > 180 {
            eprintln!("timeout esperando 'bench fin'");
            return ExitCode::FAILURE;
        }
    }

    let summary = serde_json::json!({
        "steps": steps,
        "burst": burst,
        "costs": costs,
        "max_rate_sin_perdida": max_clean_rate,
        "totales": p.summary_json(),
        "test04_ok": p.counts.crc_errors == 0 && p.counts.seq_gaps == 0,
    });
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());

    // TEST-05: falla si degrada >10% contra el baseline commiteado
    if let Some(path) = baseline {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(base) = serde_json::from_str::<serde_json::Value>(&raw) {
                let base_rate = base["max_rate_sin_perdida"].as_u64().unwrap_or(0);
                if base_rate > 0 && (max_clean_rate as f64) < base_rate as f64 * 0.9 {
                    eprintln!(
                        "degradacion >10%: {max_clean_rate} vs baseline {base_rate}"
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let render = arg_value(&args, "--render")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0usize);
    match args.first().map(String::as_str) {
        Some("decode-file") => match args.get(1) {
            Some(path) => cmd_decode_file(path, render),
            None => {
                eprintln!("uso: molectl decode-file <ruta> [--render N]");
                ExitCode::FAILURE
            }
        },
        Some("ports") => cmd_ports(),
        Some("watch") => match args.get(1) {
            Some(port) => {
                let baud = arg_value(&args, "--baud")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(921_600);
                cmd_watch(port, baud, render.max(50))
            }
            None => {
                eprintln!("uso: molectl watch <puerto> [--baud N]");
                ExitCode::FAILURE
            }
        },
        Some("bench-decoder") => {
            let frames = arg_value(&args, "--frames")
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000usize);
            cmd_bench_decoder(frames)
        }
        Some("bench") => match args.get(1) {
            Some(port) => {
                let baud = arg_value(&args, "--baud")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(921_600);
                cmd_bench(port, baud, arg_value(&args, "--baseline"))
            }
            None => {
                eprintln!("uso: molectl bench <puerto> [--baud N] [--baseline ruta]");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("subcomandos: decode-file | watch | ports | bench-decoder");
            ExitCode::FAILURE
        }
    }
}
