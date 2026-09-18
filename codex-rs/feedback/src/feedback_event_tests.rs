//! Feedback event contracts for caller-provided titles and independent report grouping.

use crate::CodexFeedback;
use pretty_assertions::assert_eq;
use sentry::protocol::Event;
use sentry::protocol::Exception;
use sentry::protocol::Level;
use std::collections::BTreeMap;

#[test]
fn custom_titles_preserve_comments_and_keep_submissions_separate() {
    let snapshot = CodexFeedback::new().snapshot(/*session_id*/ None);
    let tags = BTreeMap::from([
        (
            "feedback_title".to_string(),
            " \t\u{2003}Workflow  feedback\nwith 🌍 context\r\n ".to_string(),
        ),
        ("entrypoint".to_string(), "task-picker".to_string()),
    ]);
    let title = "Workflow  feedback\nwith 🌍 context";
    for reason in [
        Some("  The\nworkflow\twas\u{2003}useful 🌍.  "),
        Some(""),
        None,
    ] {
        let event =
            snapshot.feedback_event("other", reason, Some(&tags), /*session_source*/ None);
        let mut expected_tags = tags.clone();
        expected_tags.extend([
            ("thread_id".to_string(), snapshot.thread_id.clone()),
            ("classification".to_string(), "other".to_string()),
            (
                "cli_version".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ),
        ]);
        if let Some(reason) = reason {
            expected_tags.insert("reason".to_string(), reason.to_string());
        }
        assert_eq!(
            event,
            Event {
                event_id: event.event_id,
                timestamp: event.timestamp,
                level: Level::Info,
                message: Some(title.to_string()),
                exception: reason
                    .map(|reason| Exception {
                        ty: title.to_string(),
                        value: Some(reason.to_string()),
                        ..Default::default()
                    })
                    .into_iter()
                    .collect::<Vec<_>>()
                    .into(),
                tags: expected_tags,
                fingerprint: vec![event.event_id.to_string().into()].into(),
                ..Default::default()
            }
        );
        let next_event =
            snapshot.feedback_event("other", reason, Some(&tags), /*session_source*/ None);
        assert_ne!(event.fingerprint, next_event.fingerprint);
    }
}

#[test]
fn missing_or_blank_titles_preserve_session_titles_and_default_grouping() {
    let snapshot = CodexFeedback::new().snapshot(/*session_id*/ None);
    let reason = "  Feedback\nwith 🌍 context.  ";
    for custom_title in [None, Some(""), Some(" \t\n\u{2003} ")] {
        let tags = custom_title
            .map(|title| BTreeMap::from([("feedback_title".to_string(), title.to_string())]));
        let event = snapshot.feedback_event(
            "bug",
            Some(reason),
            tags.as_ref(),
            /*session_source*/ None,
        );
        let title = format!("[Bug]: Codex session {}", snapshot.thread_id);
        let mut expected_tags = tags.unwrap_or_default();
        expected_tags.extend([
            ("thread_id".to_string(), snapshot.thread_id.clone()),
            ("classification".to_string(), "bug".to_string()),
            (
                "cli_version".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ),
            ("reason".to_string(), reason.to_string()),
        ]);
        assert_eq!(
            event,
            Event {
                event_id: event.event_id,
                timestamp: event.timestamp,
                level: Level::Error,
                message: Some(title.clone()),
                exception: vec![Exception {
                    ty: title,
                    value: Some(reason.to_string()),
                    ..Default::default()
                }]
                .into(),
                tags: expected_tags,
                ..Default::default()
            }
        );
    }
}
