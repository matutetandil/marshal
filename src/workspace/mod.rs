// Workspace module: the core domain of the tool.
//
// This module contains the types and logic for workspaces, manifests, state
// declarations, and scope inference. It is deliberately separated from CLI
// and git interaction concerns.

pub mod manifest;
pub mod preflight;
pub mod scope;
pub mod staged;
pub mod state;

pub use manifest::Manifest;
pub use state::StateDeclaration;
