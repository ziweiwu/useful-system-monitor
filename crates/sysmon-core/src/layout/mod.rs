//! Layout, as pure functions of `(size, data)`.
//!
//! This module computes *how many rows and cells* each part of the screen gets;
//! it draws nothing. That separation is enforced by the dependency graph — this
//! crate cannot reach a terminal — which is what makes the whole size sweep
//! runnable as a unit test in milliseconds, and what makes `rows_used()` a
//! number that means something.
//!
//! Most of this app's shipped layout bugs were a constant counted but never
//! checked. The answer is that every plan exposes what it costs, and one
//! property is asserted over the entire size space: **a plan never uses more
//! rows or cells than it was given**.

pub mod budget;
pub mod columns;
pub mod overview;
pub mod screens;
pub mod scroll;
