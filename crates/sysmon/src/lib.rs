//! `useful-system-monitor`, in Rust.
//!
//! Exposed as a library as well as a binary so the frame builder — which is a
//! pure function of `(data, ui, size)` — can be swept across every terminal
//! size by an ordinary test, with no pty and no terminal. The pty harness then
//! only has to answer the question a pure test cannot: whether the built
//! binary actually mounts.

pub mod collect;
pub mod oneshot;
pub mod render;
pub mod runtime;
pub mod tui;

pub const HELP: &str = include_str!("help.txt");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BRAND: &str = "useful-system-monitor";
