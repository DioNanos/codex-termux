//! Covers effort selection and persistent-context transitions.

use super::*;
use crate::context::world_state::WorldState;
use pretty_assertions::assert_eq;

#[test]
fn persistent_instructions_follow_effort_and_catalog_updates_without_duplicates() {
    let mut history = Vec::new();
    let mut previous = None;
    let persistent = Some(ReasoningEffort::Persistent);
    let medium = Some(ReasoningEffort::Medium);
    let replacement = format!("{REPLACEMENT_NOTICE}\n\nupdated instructions");

    for (effort, instructions, expected) in [
        (None, "", None),
        (persistent.clone(), "instructions", Some("instructions")),
        (persistent.clone(), "instructions", None),
        (
            persistent.clone(),
            "updated instructions",
            Some(replacement.as_str()),
        ),
        (persistent.clone(), "", Some(REMOVAL_NOTICE)),
        (persistent.clone(), "", None),
        (persistent, "instructions", Some("instructions")),
        (medium.clone(), "", Some(REMOVAL_NOTICE)),
        (medium, "", None),
    ] {
        let mut world_state = WorldState::default();
        world_state.add_section(
            PersistentModeState::new(
                "test-model",
                effort.as_ref(),
                instructions,
                /*send_user_message_async_available*/ false,
            )
            .expect("test instructions should be valid"),
        );
        let updates = world_state
            .render_history_diff(previous.as_ref(), &history)
            .into_iter()
            .map(ContextualUserFragment::into_boxed_response_item)
            .collect::<Vec<_>>();
        assert_eq!(
            updates,
            expected
                .map(|instructions| {
                    ContextualUserFragment::into(PersistentModeState {
                        instructions: instructions.to_string(),
                    })
                })
                .into_iter()
                .collect::<Vec<_>>()
        );
        history.extend(updates);
        previous = Some(world_state.snapshot());
    }
}

#[test]
fn retained_persistent_instructions_are_replaced_or_retired_without_a_snapshot() {
    let retained = ContextualUserFragment::into(PersistentModeState {
        instructions: "previous instructions".to_string(),
    });
    for (effort, expected) in [
        (
            ReasoningEffort::Persistent,
            format!("{REPLACEMENT_NOTICE}\n\ncurrent instructions"),
        ),
        (ReasoningEffort::Medium, REMOVAL_NOTICE.to_string()),
    ] {
        let mut world_state = WorldState::default();
        world_state.add_section(
            PersistentModeState::new(
                "test-model",
                Some(&effort),
                "current instructions",
                /*send_user_message_async_available*/ false,
            )
            .expect("test instructions should be valid"),
        );
        assert_eq!(
            world_state
                .render_history_diff(/*previous*/ None, std::slice::from_ref(&retained))
                .into_iter()
                .map(ContextualUserFragment::into_boxed_response_item)
                .collect::<Vec<_>>(),
            vec![ContextualUserFragment::into(PersistentModeState {
                instructions: expected,
            })]
        );
    }
}

#[test]
fn persistent_instructions_reject_oversized_values() {
    let oversized = "x".repeat(8 * 1024 + 1);
    let error = PersistentModeState::new(
        "test-model",
        Some(&ReasoningEffort::Persistent),
        oversized.as_str(),
        /*send_user_message_async_available*/ false,
    )
    .expect_err("oversized persistent instructions must be rejected");

    assert_eq!(error.field, "persistent_instructions");
    assert_eq!(error.model_slug, "test-model");
    assert_eq!(error.actual_bytes, 8 * 1024 + 1);
    assert_eq!(error.max_bytes, 8 * 1024);
}

#[test]
fn persistent_instructions_preserve_empty_none_and_exact_limit() {
    let bundled = codex_prompts::ResolvedModelMessages::bundled()
        .persistent_instructions()
        .trim()
        .to_string();
    let built_in = PersistentModeState::new(
        "test-model",
        Some(&ReasoningEffort::Persistent),
        &bundled,
        false,
    )
    .expect("bundled instructions should be valid");
    assert_eq!(
        built_in.body().trim(),
        bundled.replace("{{ approval_request_channel }}", "")
    );
    assert!(
        PersistentModeState::new("test-model", Some(&ReasoningEffort::Persistent), "", false,)
            .expect("empty instructions should disable the section")
            .body()
            .trim()
            .is_empty()
    );

    let exact = "x".repeat(8 * 1024);
    let state = PersistentModeState::new(
        "test-model",
        Some(&ReasoningEffort::Persistent),
        exact.as_str(),
        false,
    )
    .expect("8 KiB instructions should pass");
    assert_eq!(state.body().trim().len(), 8 * 1024);
}

#[test]
fn persistent_instructions_validate_after_placeholder_rendering() {
    let placeholder = "{{ approval_request_channel }}";
    let source = format!(
        "{}{}",
        "x".repeat(8 * 1024 - placeholder.len()),
        placeholder
    );
    let error = PersistentModeState::new(
        "test-model",
        Some(&ReasoningEffort::Persistent),
        source.as_str(),
        true,
    )
    .expect_err("placeholder expansion over the cap must be rejected");
    assert_eq!(error.field, "persistent_instructions");
    assert!(error.actual_bytes > 8 * 1024);
}
