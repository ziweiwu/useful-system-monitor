//! The dashboard's state and keymap, as pure data and pure functions.
//!
//! Nothing here draws or performs I/O: a key goes in, a new state and a list of
//! effects come out. That is what makes the input fuzzer able to drive millions
//! of events with no terminal, and it is what removes the stale-closure bug
//! class outright — the reducer is applied one event at a time with the state
//! updated between each, so a burst cannot be handled against a stale view of
//! the mode.

pub mod input;
pub mod state;
