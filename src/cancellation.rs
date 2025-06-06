use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct Cancellation {
    /// How many times did we try to cancel the CLI
    count: AtomicUsize,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_soft_cancellation(&self) -> bool {
        self.count.load(Ordering::Relaxed) == 1
    }

    pub fn is_hard_cancellation(&self) -> bool {
        self.count.load(Ordering::Relaxed) > 1
    }

    pub fn is_cancelled(&self) -> bool {
        self.count.load(Ordering::Relaxed) > 0
    }
}
