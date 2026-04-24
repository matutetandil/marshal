// Command implementations.
//
// Each submodule here implements one user-visible command. They share
// conventions: follow the detect → pre-flight → execute → report cycle,
// support --explain where applicable, respect the nine invariants.

pub mod clone;
pub mod init;
pub mod log;
pub mod passthrough;
pub mod status;
