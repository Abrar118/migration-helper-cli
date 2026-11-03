//! sidetrack core: database branches linked to git branches.
//!
//! All logic lives here so the CLI (and later the desktop app) stay thin.

pub mod apply;
pub mod compose;
pub mod config;
pub mod detect;
pub mod engine;
pub mod envfile;
pub mod git;
pub mod migrations;
pub mod naming;
pub mod plan;
pub mod planner;
pub mod process;
pub mod registry;
pub mod workspace;
