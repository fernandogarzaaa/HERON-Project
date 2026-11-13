//! Slint view logic, as pure Rust modules.
//!
//! `chat` is the Chat lane's thread state machine (`ChatView`).
//! `views` is the Views lane's view models plus the `ViewsBridge`
//! trait the Shell implements over `HeronClient`.
//! Neither module depends on Slint, so the library test suite covers
//! them. The glue that drives them from the generated `MainWindow`
//! lives in the desktop binary (`src/bin/desktop/views_slint.rs`).

pub mod chat;
pub mod views;
