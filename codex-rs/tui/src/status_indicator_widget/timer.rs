//! Pause-aware status time owned independently of the optional status row.
//! A phase can override the displayed clock without changing turn accounting.

use std::time::Duration;
use std::time::Instant;

#[derive(Debug)]
pub(crate) struct StatusTimer {
    /// Optional wall-clock origin for the displayed phase, without resetting turn accounting.
    pub(crate) display_started_at: Option<Instant>,
    pub(super) elapsed_running: Duration,
    pub(super) last_resume_at: Instant,
    pub(super) is_paused: bool,
}

impl Default for StatusTimer {
    fn default() -> Self {
        Self {
            display_started_at: None,
            elapsed_running: Duration::ZERO,
            last_resume_at: Instant::now(),
            is_paused: false,
        }
    }
}

impl StatusTimer {
    pub(crate) fn reset(&mut self, elapsed: Duration) {
        self.elapsed_running = elapsed;
        self.last_resume_at = Instant::now();
        // A turn can start while an MCP elicitation or approval is still open.
        // Reset elapsed time without changing that modal's pause state.
    }

    pub(crate) fn pause_at(&mut self, now: Instant) {
        if !self.is_paused {
            self.elapsed_running += now.saturating_duration_since(self.last_resume_at);
            self.is_paused = true;
        }
    }

    pub(crate) fn resume_at(&mut self, now: Instant) {
        if self.is_paused {
            self.last_resume_at = now;
            self.is_paused = false;
        }
    }

    pub(crate) fn elapsed_at(&self, now: Instant) -> Duration {
        self.elapsed_running
            + if self.is_paused {
                Duration::ZERO
            } else {
                now.saturating_duration_since(self.last_resume_at)
            }
    }
}

#[cfg(test)]
#[path = "timer_tests.rs"]
mod tests;
