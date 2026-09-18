use super::*;
use codex_extension_api::ExtensionData;
use codex_extension_api::TurnItemContributor;
use codex_protocol::ResponseItemId;
use codex_protocol::items::AgentMessageContent;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

struct RewriteAgentMessageContributor;

impl TurnItemContributor for RewriteAgentMessageContributor {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
        item: &'a mut TurnItem,
    ) -> codex_extension_api::ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if let TurnItem::AgentMessage(agent_message) = item {
                agent_message.content = vec![AgentMessageContent::Text {
                    text: "plan contributed assistant text".to_string(),
                }];
            }
            Ok(())
        })
    }
}

fn assistant_output_text(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: Some(ResponseItemId::with_suffix("msg", "1")),
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[derive(Clone, Default)]
struct CaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("writer lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

const ALWAYS_ON_SINK_INTEREST_CHILD_ENV: &str = "CODEX_ALWAYS_ON_SINK_INTEREST_CHILD";

/// Evaluates the production interest check (`event_enabled!`) for the
/// token-estimate target against the always-on topology, in THIS process.
/// Only meaningful in a process no other test has touched.
fn interest_under<S>(subscriber: S) -> bool
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    tracing::subscriber::with_default(subscriber, || {
        tracing::event_enabled!(
            target: POST_SAMPLING_TOKEN_ESTIMATE_TARGET,
            tracing::Level::TRACE,
            turn_id = "turn-always-on-sink-pin",
            estimated_token_count = 1u64,
            message = "estimate payload"
        )
    })
}

fn always_on_sink_estimate_interest_enabled(regressed: bool) -> bool {
    let writer = CaptureWriter::default();
    if regressed {
        // Counterexample topology: the feedback layer lost its OFF rule.
        let subscriber = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer.clone())
                    .with_filter(
                        tracing_subscriber::filter::Targets::new()
                            .with_default(tracing::level_filters::LevelFilter::TRACE),
                    ),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer.clone())
                    .with_filter(codex_state::log_db::default_filter()),
            );
        interest_under(subscriber)
    } else {
        let feedback = codex_feedback::CodexFeedback::new();
        let subscriber = tracing_subscriber::registry()
            .with(feedback.logger_layer())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer.clone())
                    .with_filter(codex_state::log_db::default_filter()),
            );
        interest_under(subscriber)
    }
}

/// Re-executes this test binary in a fresh process that runs only the pin
/// test; the child short-circuits into the interest evaluation and exits
/// with the interest verdict, free of any callsite contamination.
fn spawn_always_on_sink_interest_child(regressed: bool) -> std::process::Output {
    let exe = std::env::current_exe().expect("test binary path");
    std::process::Command::new(exe)
        .args([
            "--exact",
            "session::turn::tests::post_sampling_token_estimate_is_disabled_by_always_on_sinks",
            "--test-threads",
            "1",
            "--nocapture",
        ])
        .env(
            ALWAYS_ON_SINK_INTEREST_CHILD_ENV,
            if regressed { "1" } else { "0" },
        )
        .output()
        .expect("spawn always-on sink interest child")
}

fn capture_string(writer: &CaptureWriter, label: &str) -> String {
    String::from_utf8(writer.0.lock().expect("writer lock").clone())
        .unwrap_or_else(|_| panic!("{label} output must be utf-8"))
}

/// The always-on sinks (the feedback logger ring buffer and the log database's
/// default filter) must keep the post-sampling token-estimate payload out of
/// their output, and the interest check the production turn runtime relies on
/// (`tracing::event_enabled!` before computing the estimate in
/// `core/src/session/turn.rs`) must stay disabled so the estimate is never
/// even computed. Pin BOTH sinks — the real `CodexFeedback` ring buffer via its
/// public snapshot, plus an in-memory writer for the log-db layer — including a
/// positive control on each so the pin cannot pass vacuously. Everything runs
/// through `tracing::subscriber::with_default`, a scoped dispatcher, so the pin
/// cannot be contaminated by whichever global default subscriber other tests
/// in this crate install.
#[test]
fn post_sampling_token_estimate_is_disabled_by_always_on_sinks() {
    if let Some(mode) = std::env::var_os(ALWAYS_ON_SINK_INTEREST_CHILD_ENV) {
        // Self-reexeced child: evaluate only the interest invariant in this
        // otherwise untouched process and report it through the exit code.
        let regressed = mode == "1";
        let enabled = always_on_sink_estimate_interest_enabled(regressed);
        println!("{ALWAYS_ON_SINK_INTEREST_CHILD_ENV}=done enabled={enabled}");
        std::process::exit(i32::from(enabled));
    }

    let feedback = codex_feedback::CodexFeedback::new();
    let writer = CaptureWriter::default();
    let subscriber = tracing_subscriber::registry()
        .with(feedback.logger_layer())
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer.clone())
                .with_filter(codex_state::log_db::default_filter()),
        );

    // The interest check the production turn runtime performs before spending
    // work on the estimate. `event_enabled!` consults process-wide callsite
    // interest, which other tests in this crate rebuild against their own
    // global subscribers, so an in-process evaluation depends on test order
    // (verified: green alone, red after conflicting_ready_environment…).
    // The audit's requested isolation is at the harness/process level: the
    // invariant runs in a fresh child of this test binary, where no other
    // test has touched callsite interest yet.
    let interest_child = spawn_always_on_sink_interest_child(/*regressed*/ false);
    let child_stdout = String::from_utf8_lossy(&interest_child.stdout).to_string();
    assert!(
        interest_child.status.success(),
        "clean-process interest check failed ({}): {}{}",
        interest_child.status,
        child_stdout,
        String::from_utf8_lossy(&interest_child.stderr),
    );
    assert!(
        child_stdout.contains(&format!("{ALWAYS_ON_SINK_INTEREST_CHILD_ENV}=done")),
        "the interest child must actually run (got: {child_stdout})"
    );

    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(
            target: POST_SAMPLING_TOKEN_ESTIMATE_TARGET,
            tracing::Level::TRACE,
            turn_id = "turn-always-on-sink-pin",
            estimated_token_count = 1u64,
            message = "estimate payload"
        );
        // Positive control: the same layered subscriber must capture an
        // ordinary event on both sinks, proving they are wired up.
        tracing::event!(
            target: "codex_core::always_on_sink_probe",
            tracing::Level::INFO,
            "always-on sink probe"
        );
    });

    let captured = capture_string(&writer, "log-db sink");
    assert!(
        !captured.contains("estimate payload"),
        "always-on log-db sink must not capture the token-estimate payload, got: {captured}"
    );
    assert!(
        captured.contains("always-on sink probe"),
        "positive control: the log-db sink must capture an ordinary event, got: {captured}"
    );

    let feedback_logs = String::from_utf8(feedback.snapshot(None).log_attachment(None).buffer)
        .expect("feedback ring buffer is utf-8");
    assert!(
        !feedback_logs.contains("estimate payload"),
        "always-on feedback sink must not capture the token-estimate payload, got: {feedback_logs}"
    );
    assert!(
        feedback_logs.contains("always-on sink probe"),
        "positive control: the feedback sink must capture an ordinary event, got: {feedback_logs}"
    );
}

/// Counterexample topology from the independent audit: a feedback layer that
/// regresses by losing its OFF rule for the estimate target (default TRACE
/// with no exclusion) re-enables the production `event_enabled!` check and
/// leaks the estimate payload into the feedback sink while the log-db sink
/// keeps filtering. Pinning that the regression produces exactly the signals
/// the test above forbids proves the pin fails on this regression instead of
/// passing vacuously.
#[test]
fn always_on_sink_pin_detects_a_regressed_feedback_filter() {
    let regressed_feedback_writer = CaptureWriter::default();
    let db_writer = CaptureWriter::default();
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(regressed_feedback_writer.clone())
                .with_filter(
                    tracing_subscriber::filter::Targets::new()
                        .with_default(tracing::level_filters::LevelFilter::TRACE),
                ),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(db_writer.clone())
                .with_filter(codex_state::log_db::default_filter()),
        );

    // In the regressed topology the same clean-process interest check must
    // flip to enabled: the child exits non-zero exactly when the pin's
    // invariant is violated, proving the pin fires on this regression.
    let interest_child = spawn_always_on_sink_interest_child(/*regressed*/ true);
    assert!(
        !interest_child.status.success(),
        "the regressed topology must re-enable the estimate target, otherwise the pin above is not testing the feedback filter"
    );

    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(
            target: POST_SAMPLING_TOKEN_ESTIMATE_TARGET,
            tracing::Level::TRACE,
            turn_id = "turn-always-on-sink-pin",
            estimated_token_count = 1u64,
            message = "estimate payload"
        );
    });

    let feedback_captured = capture_string(&regressed_feedback_writer, "regressed feedback sink");
    assert!(
        feedback_captured.contains("estimate payload"),
        "regressed topology must leak the payload into the feedback sink: \
         otherwise the pin above cannot catch this regression"
    );
    let db_captured = capture_string(&db_writer, "log-db sink");
    assert!(
        !db_captured.contains("estimate payload"),
        "the log-db filter must keep excluding the estimate payload even when the feedback layer regresses, got: {db_captured}"
    );
}

#[tokio::test]
async fn plan_mode_uses_contributed_turn_item_for_last_agent_message() {
    let (mut session, turn_context) = crate::session::tests::make_session_and_context().await;
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::new();
    builder.turn_item_contributor(Arc::new(RewriteAgentMessageContributor));
    session.services.extensions = Arc::new(builder.build());
    let turn_store = ExtensionData::new(turn_context.sub_id.clone());
    let mut state = PlanModeStreamState::new(&turn_context.sub_id);
    let mut last_agent_message = None;
    let item = assistant_output_text("original assistant text");

    let step_context = StepContext::for_test(Arc::new(turn_context));
    let handled = handle_assistant_item_done_in_plan_mode(
        &session,
        &step_context,
        &turn_store,
        &item,
        &mut state,
        /*previously_active_item*/ None,
        &mut last_agent_message,
    )
    .await;

    assert!(handled);
    assert_eq!(
        last_agent_message.as_deref(),
        Some("plan contributed assistant text")
    );
}

#[test]
fn realtime_user_verification_notice_excludes_request_payload() {
    let event = EventMsg::ElicitationRequest(codex_protocol::approvals::ElicitationRequestEvent {
        turn_id: None,
        server_name: "private-server-name".to_string(),
        id: codex_protocol::mcp::RequestId::String("private-request-id".to_string()),
        request: codex_protocol::approvals::ElicitationRequest::UserVerification {
            title: "private-title".to_string(),
            description: "private-description".to_string(),
            challenge: "private-challenge".to_string(),
        },
    });
    assert_eq!(
        realtime_text_for_event(&event),
        Some((
            "<user_verification_notice>User verification is required. Please respond in the app.</user_verification_notice>".to_string(),
            None,
        )),
    );
}
