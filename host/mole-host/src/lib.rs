//! mole-host — el pipeline del host (ARQ-02/ARQ-03, HOST-01..14).
//!
//! La biblioteca que comparten `molectl` (headless) y `mole-app` (Tauri):
//! HOST-14 exige que todas las ventanas consuman el mismo contrato, y la
//! forma de garantizarlo es que haya un solo pipeline.

pub mod pipeline;
pub mod transport;

pub use pipeline::{Counts, Pipeline};
pub use transport::{FileReplay, SerialTransport, Transport};
