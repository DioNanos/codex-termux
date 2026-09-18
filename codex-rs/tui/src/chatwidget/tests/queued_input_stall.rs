use super::*;

/// A local compaction is opened with the pending-start latch but it is not a
/// user turn: nothing sends `TaskStarted`/`TurnComplete` for it. While that
/// latch stays high with no turn running, everything that depends on it stops
/// at once: a submission is queued instead of starting a turn, the queue can
/// never be drained, and a loop tick is refused (`BlockedUserTurn`).
#[tokio::test]
async fn an_orphan_pending_start_queues_input_and_refuses_to_drain() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.input_queue.user_turn_pending_start = true; // orphan latch: no turn running
    chat.queue_user_message(UserMessage::from("follow-up"));
    assert_eq!(
        chat.input_queue.queued_user_messages.len(),
        1,
        "with the latch stuck the submission is queued instead of starting a turn"
    );
    assert!(
        !chat.maybe_send_next_queued_input(),
        "with the latch stuck the queue is never drained"
    );
    assert_eq!(chat.input_queue.queued_user_messages.len(), 1);
}

/// The regression this fix closes: a local compaction sets the pending-start
/// latch (`submit_op` -> `prepare_local_op_submission`) and its completion
/// arrives as `ContextCompacted`, which used to be a no-op. Nothing else was
/// going to clear the latch, so the composer stayed frozen until some
/// unrelated turn happened to start.
#[tokio::test]
async fn a_local_compaction_does_not_leave_the_pending_start_behind() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    assert!(
        chat.submit_op(AppCommand::Compact),
        "the local op must reach the op target"
    );
    assert!(
        chat.input_queue.user_turn_pending_start,
        "the local op opens with the pending-start latch"
    );

    chat.handle_server_notification(
        ServerNotification::ContextCompacted(
            serde_json::from_value(serde_json::json!({
                "threadId": thread_id.to_string(),
                "turnId": "turn-1",
            }))
            .expect("compaction payload"),
        ),
        /*replay_kind*/ None,
    );

    assert!(
        !chat.input_queue.user_turn_pending_start,
        "a local compaction is not a user turn: it must not leave the latch high"
    );
}

/// The terminal-turn boundary is the defensive net: an orphan latch (set by a
/// path whose turn never started) must not survive it. This base provides
/// `finalize_turn` at that boundary (the fork-specific
/// `reconcile_terminal_turn` wrapper does not exist here).
#[tokio::test]
async fn a_terminal_turn_clears_an_orphan_pending_start() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.input_queue.user_turn_pending_start = true;
    chat.queue_user_message(UserMessage::from("follow-up"));
    chat.finalize_turn();
    assert!(
        !chat.input_queue.user_turn_pending_start,
        "a terminal turn must not leave a pending start behind with no turn running"
    );
}
