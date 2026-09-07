//! Text measurement and sanitisation — the one place that decides how wide
//! anything is. Every truncate, pad and wrap in the app routes through here, so
//! the rule cannot disagree with itself. See I-19.

pub mod sanitize;
pub mod width;

pub use sanitize::sanitize_text;
pub use width::{cells, display_width, pad_end, pad_start, truncate, wrap_to_width};
