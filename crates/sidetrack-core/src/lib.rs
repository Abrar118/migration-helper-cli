//! sidetrack core: database branches linked to git branches.
//!
//! All logic lives here so the CLI (and later the desktop app) stay thin.

pub mod compose;
pub mod config;
pub mod engine;
pub mod envfile;
pub mod git;
pub mod naming;
pub mod process;
pub mod registry;
