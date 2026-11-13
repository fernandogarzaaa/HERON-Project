//! Heron — a local-first personal AI agent.
pub mod agent;
pub mod ambient;
pub mod app;
pub mod browser;
pub mod cli_llm;
pub mod config;
pub mod embed;
pub mod eval;
pub mod llm;
pub mod mail;
pub mod mcp;
pub mod memory;
pub mod permissions;
pub mod persona;
pub mod sandbox;
pub mod schedule;
pub mod server;
pub mod skills;
pub mod sources;
pub mod store;
pub mod telegram;
pub mod tools;
/// Slint view logic: the Chat and Views lane state machines.
/// These are pure Rust (no Slint dependency) so `cargo test --lib`
/// covers them. The desktop binary's `views_slint.rs` glue drives
/// them through the generated MainWindow.
pub mod ui;
pub mod usage;
pub mod util;
pub mod workflow;
