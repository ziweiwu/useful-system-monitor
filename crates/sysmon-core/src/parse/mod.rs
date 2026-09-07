//! Pure parsers for the collectors' command output. Kept separate from process
//! spawning so every rule is testable against the captured fixtures in
//! `test/fixtures/`, which the Rust tests read verbatim.

pub mod battery;
pub mod df;
pub mod linux;
pub mod ps;
pub mod top_power;
pub mod vm_stat;
