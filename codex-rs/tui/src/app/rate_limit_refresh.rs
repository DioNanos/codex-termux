//! Orders full usage reads and coalesces recovery refreshes within an account/limit generation.
//! Periodic reads share that ordering and wait while another periodic read or recovery is pending.

use crate::app_event::RateLimitRefreshOrigin;
use std::time::Duration;
use std::time::Instant;

#[derive(Default)]
pub(super) struct RateLimitRefreshState {
    next_id: u64,
    applied_id: u64,
    recovery: Option<PendingRecovery>,
    periodic: Option<(u64, u64)>,
    last_requested_at: Option<Instant>,
}

struct PendingRecovery {
    request: (u64, u64),
    origin: RateLimitRefreshOrigin,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RateLimitRefreshOutcome {
    Apply,
    Ignore,
    RefreshRecovery,
}

#[derive(PartialEq, Eq)]
pub(super) enum RateLimitReadStatus {
    Succeeded,
    Failed,
}

impl RateLimitRefreshState {
    pub(super) fn poll_deadline(&self, interval: Duration) -> Option<Instant> {
        if self.periodic.is_some() || self.recovery.is_some() {
            return None;
        }
        Some(
            self.last_requested_at
                .map(|last| last + interval)
                .unwrap_or_else(Instant::now),
        )
    }

    pub(super) fn has_pending_recovery(&self) -> bool {
        self.recovery.is_some()
    }

    pub(super) fn start(
        &mut self,
        origin: RateLimitRefreshOrigin,
        generation: &mut u64,
    ) -> Option<(u64, u64)> {
        if origin == RateLimitRefreshOrigin::Periodic
            && (self.periodic.is_some() || self.recovery.is_some())
        {
            return None;
        }
        if matches!(origin, RateLimitRefreshOrigin::Recovery) {
            // A new hard error supersedes a post-reset read that started before the error.
            if self
                .recovery
                .as_ref()
                .is_some_and(|pending| pending.origin == RateLimitRefreshOrigin::Recovery)
            {
                return None;
            }
            // A hard error invalidates even an older read without a preceding rolling notification.
            *generation = generation.wrapping_add(1);
        }
        self.next_id = self.next_id.wrapping_add(1);
        let request = (self.next_id, *generation);
        self.last_requested_at = Some(Instant::now());
        if origin == RateLimitRefreshOrigin::Periodic {
            self.periodic = Some(request);
        }
        // The post-reset read takes over the hold from the invalidated recovery request.
        if matches!(
            origin,
            RateLimitRefreshOrigin::Recovery | RateLimitRefreshOrigin::ResetConsume { .. }
        ) {
            self.recovery = Some(PendingRecovery { request, origin });
        }
        Some(request)
    }

    // Account changes and successful resets invalidate the need as well as the old response.
    pub(super) fn invalidate_recovery(&mut self) {
        self.recovery = None;
        self.periodic = None;
        self.last_requested_at = None;
    }

    pub(super) fn finish(
        &mut self,
        request_id: u64,
        generation: u64,
        current_generation: u64,
        status: RateLimitReadStatus,
    ) -> RateLimitRefreshOutcome {
        if self.periodic == Some((request_id, generation)) {
            self.periodic = None;
        }
        if generation == current_generation && request_id == self.next_id {
            // Completion starts the next wait, including after failures/timeouts, so errors
            // cannot create a tight retry loop while the account is nearly exhausted.
            self.last_requested_at = Some(Instant::now());
        }
        if self
            .recovery
            .as_ref()
            .is_some_and(|pending| pending.request == (request_id, generation))
        {
            self.recovery = None;
            // A rolling hard stop can arrive after the error, without another error notification.
            if generation != current_generation {
                return RateLimitRefreshOutcome::RefreshRecovery;
            }
        }
        if generation != current_generation
            || request_id < self.applied_id
            || status == RateLimitReadStatus::Failed
        {
            return RateLimitRefreshOutcome::Ignore;
        }
        self.applied_id = request_id;
        RateLimitRefreshOutcome::Apply
    }
}
