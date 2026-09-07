//! Kill safety. Every rule here is pure, so every refusal path is testable
//! without spawning or signalling anything.

pub mod guards;
pub mod signal;
