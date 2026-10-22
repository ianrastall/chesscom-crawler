//! Cooperative stop signal with interruptible waits.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Control {
    stopped: Mutex<bool>,
    wake: Condvar,
}

/// Returned when a wait or request is abandoned because a stop was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped;

impl Control {
    pub fn request_stop(&self) {
        *self.stopped.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        *self.stopped.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits for `duration` unless a stop is requested first.
    pub fn sleep(&self, duration: Duration) -> Result<(), Stopped> {
        let deadline = Instant::now() + duration;
        let mut stopped = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if *stopped {
                return Err(Stopped);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            stopped = self.wake.wait_timeout(stopped, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    pub fn check(&self) -> Result<(), Stopped> {
        if self.is_stopped() { Err(Stopped) } else { Ok(()) }
    }
}
