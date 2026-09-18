//! Live compaction status. Its wall clock is separate from the turn's running time,
//! and only a matching live completion contributes a duration to the transcript.

use super::*;

pub(super) const COMPACTION_HEADER: &str = "Compacting context";
pub(super) const COMPACTION_DETAILS: &str = "Making room to continue.";

#[derive(Debug)]
pub(super) struct ActiveCompaction {
    pub(super) id: String,
    pub(super) started_at: Instant,
}

impl ChatWidget {
    pub(super) fn on_context_compaction_started(&mut self, id: String, elapsed: Duration) {
        if self
            .status_state
            .compaction
            .as_ref()
            .is_some_and(|active| active.id == id)
        {
            return;
        }
        self.flush_answer_stream_with_separator();
        let now = Instant::now();
        let started_at = now.checked_sub(elapsed).unwrap_or(now);
        self.status_state.compaction = Some(ActiveCompaction { id, started_at });
        self.bottom_pane.set_status_timer_origin(Some(started_at));
        self.bottom_pane.ensure_status_indicator();
        self.set_status_header(COMPACTION_HEADER.to_string());
    }

    /// A local compaction is opened with the pending-start latch (`submit_op` ->
    /// `prepare_local_op_submission`) but it is NOT a user turn: nothing sends
    /// `TaskStarted`/`TurnComplete` for it, and its completion arrives as
    /// `ContextCompacted`. Left alone, that latch stays high with no turn
    /// running and freezes the composer: every later submission is queued
    /// instead of starting a turn, the queue can never be drained, and a loop
    /// tick is refused as `BlockedUserTurn`. Clear it at the boundary that
    /// abandons it and let the queue drain again.
    pub(super) fn on_context_compacted_notification(&mut self) {
        if !self.input_queue.user_turn_pending_start || self.turn_lifecycle.agent_turn_running {
            return;
        }
        self.input_queue.user_turn_pending_start = false;
        tracing::warn!("cleared an orphan pending user-turn start after a local compaction");
        self.refresh_pending_input_preview();
        let _ = self.maybe_send_next_queued_input();
    }

    pub(super) fn clear_context_compaction(&mut self) {
        if self.status_state.compaction.take().is_some() {
            self.bottom_pane
                .set_status_timer_origin(/*started_at*/ None);
            self.set_status_header("Working".to_string());
        }
    }

    pub(super) fn on_context_compaction_completed(&mut self, id: &str, from_replay: bool) {
        let mut message = "Context compacted".to_string();
        if let Some(active) = self.status_state.compaction.as_ref()
            && active.id == id
        {
            if !from_replay {
                let elapsed = crate::status_indicator_widget::fmt_elapsed_compact(
                    active.started_at.elapsed().as_secs(),
                );
                message = format!("Context compacted · {elapsed}");
            }
            self.clear_context_compaction();
        }
        self.add_info_message(message, /*hint*/ None);
    }
}
