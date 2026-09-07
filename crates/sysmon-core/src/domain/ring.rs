//! Fixed-capacity ring buffer. All history uses this so memory never grows with
//! uptime. See I-10.

/// A circular buffer of `f64`, overwriting the oldest value when full.
#[derive(Debug, Clone)]
pub struct Ring {
    buf: Vec<f64>,
    len: usize,
    head: usize,
}

/// A slot no sample has reached yet.
const EMPTY: f64 = 0.0;

impl Ring {
    /// # Panics
    /// If `capacity` is zero. A zero-capacity history is a programming error,
    /// not a runtime condition: every caller passes a constant.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        Self {
            buf: vec![EMPTY; capacity],
            len: 0,
            head: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn push(&mut self, value: f64) {
        self.buf[self.head] = value;
        self.head = (self.head + 1) % self.buf.len();
        if self.len < self.buf.len() {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Oldest to newest.
    pub fn to_vec(&self) -> Vec<f64> {
        let cap = self.buf.len();
        let start = (self.head + cap - self.len) % cap;
        (0..self.len).map(|i| self.buf[(start + i) % cap]).collect()
    }

    pub fn last(&self) -> Option<f64> {
        if self.len == 0 {
            None
        } else {
            Some(self.buf[(self.head + self.buf.len() - 1) % self.buf.len()])
        }
    }
}
