//! mole-host — el pipeline del host (ARQ-02/ARQ-03, HOST-01..14).
//!
//! La biblioteca que comparten `molectl` (headless) y `mole-app` (Tauri):
//! HOST-14 exige que todas las ventanas consuman el mismo contrato, y la
//! forma de garantizarlo es que haya un solo pipeline.

pub mod filter;
pub mod pipeline;
pub mod store;
pub mod tick;
pub mod transport;
pub mod watch;

pub use filter::{filter_indices, LogFilter, TextFilter};
pub use pipeline::{render_log, Counts, Pipeline};
pub use store::{LogStore, Retention, RowKind};
pub use regex::Regex;
pub use tick::{log_slice_raw, make_tick, query, TickState};
pub use transport::{known_vid, list_ports, FileReplay, SerialTransport, Transport};
pub use watch::{WatchStats, WatchStore};
