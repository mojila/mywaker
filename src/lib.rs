//! # mywaker
//!
//! Run a team of AI employees backed by OpenCode from your terminal.
//!
//! The crate is split into a thin `main.rs` (CLI + lifecycle) and library
//! modules organized by feature.

#![deny(clippy::correctness)]

pub mod app;
pub mod config;
pub mod event;
pub mod harness;
pub mod jev;
pub mod logging;
pub mod message;
pub mod opencode;
pub mod projects;
pub mod role;
pub mod skills;
pub mod team;
pub mod tui;
pub mod ui;
pub mod usage;
pub mod workflow;
