//! The dashboard screens, in the order the tab strip shows them.
//!
//! The order is the navigation order: left/right step through this list, and the
//! number key is the 1-based position, so the strip on screen and the keymap
//! cannot drift apart. See I-27.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Overview,
    Cpu,
    Memory,
    Battery,
    Disk,
}

pub const VIEW_ORDER: [View; 5] = [
    View::Overview,
    View::Cpu,
    View::Memory,
    View::Battery,
    View::Disk,
];

impl View {
    /// Short enough that all five fit on one line at the 80-column minimum.
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "OVERVIEW",
            Self::Cpu => "CPU",
            Self::Memory => "MEMORY",
            Self::Battery => "BATTERY",
            Self::Disk => "DISK",
        }
    }

    /// The number key that jumps straight to this view, derived from the order
    /// so the two can never disagree.
    pub fn key(self) -> char {
        let i = VIEW_ORDER.iter().position(|v| *v == self).unwrap_or(0);
        char::from_digit(i as u32 + 1, 10).unwrap_or('1')
    }

    /// The view a number key selects, if any.
    pub fn from_key(key: char) -> Option<Self> {
        let i = key.to_digit(10)?;
        if i == 0 {
            return None;
        }
        VIEW_ORDER.get(i as usize - 1).copied()
    }

    /// Step `delta` views along the strip, wrapping at both ends.
    ///
    /// Wrapping matters more than it looks: a tab strip whose arrow key
    /// silently does nothing at the last tab reads as a broken key, not as a
    /// boundary.
    pub fn step(self, delta: i32) -> Self {
        let n = VIEW_ORDER.len() as i32;
        let i = VIEW_ORDER.iter().position(|v| *v == self).unwrap_or(0) as i32;
        VIEW_ORDER[(((i + delta) % n + n) % n) as usize]
    }
}
