//! The pure core of `useful-system-monitor`.
//!
//! This crate deliberately depends on no terminal, no renderer and no process
//! spawning. Layout, parsing, guards and domain rules live here as pure
//! functions of plain data; the binary crate supplies I/O and drawing. The
//! dependency graph is what enforces that separation, so "layout is a pure
//! function of (size, data)" cannot quietly stop being true.

pub mod app;
pub mod cli;
pub mod domain;
pub mod kill;
pub mod layout;
pub mod parse;
pub mod text;
pub mod ui;
