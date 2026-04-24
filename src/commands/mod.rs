//! Command implementations.
//!
//! - `passthrough` is the 0.1.0 core: forward argv to git byte-exact.
//! - `config` is the `marshal config` dispatcher (step 5a+).
//! - `clone`, `init`, `log`, `status` are Phase 2 scaffolds not wired to `main`
//!   in 0.2.x; they live here so the design surface remains visible.

pub mod clone;
pub mod config;
pub mod init;
pub mod log;
pub mod passthrough;
pub mod status;
