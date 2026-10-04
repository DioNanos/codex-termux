//! Covers persistent-context transitions independently of effort selection.

use super::*;
use crate::context::world_state::WorldState;
use pretty_assertions::assert_eq;

#[test]
fn persistent_instructions_follow_mode_and_catalog_updates_without_duplicates() {
    let mut history = Vec::new();
    let mut previous = None;
    let replacement = format!("{REPLACEMENT_NOTICE}\n\nupdated instructions");

    for (enabled, instructions, expected) in [
        (false, "", None),
        (true, "instructions", Some("instructions")),
        (true, "instructions", None),
        (true, "updated instructions", Some(replacement.as_str())),
        (true, "", Some(REMOVAL_NOTICE)),
        (true, "", None),
        (true, "instructions", Some("instructions")),
        (false, "", Some(REMOVAL_NOTICE)),
        (false, "", None),
    ] {
        let mut world_state = WorldState::default();
        world_state.add_section(
            PersistentModeState::new(
                "test-model",
                enabled,
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
    for (enabled, expected) in [
        (
            true,
            format!("{REPLACEMENT_NOTICE}\n\ncurrent instructions"),
        ),
        (false, REMOVAL_NOTICE.to_string()),
    ] {
        let mut world_state = WorldState::default();
        world_state.add_section(
            PersistentModeState::new(
                "test-model",
                enabled,
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
        true,
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
    let built_in = PersistentModeState::new("test-model", true, &bundled, false)
        .expect("bundled instructions should be valid");
    assert_eq!(
        built_in.body().trim(),
        bundled.replace("{{ approval_request_channel }}", "")
    );
    assert!(
        PersistentModeState::new("test-model", true, "", false,)
            .expect("empty instructions should disable the section")
            .body()
            .trim()
            .is_empty()
    );

    let exact = "x".repeat(8 * 1024);
    let state = PersistentModeState::new("test-model", true, exact.as_str(), false)
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
    let error = PersistentModeState::new("test-model", true, source.as_str(), true)
        .expect_err("placeholder expansion over the cap must be rejected");
    assert_eq!(error.field, "persistent_instructions");
    assert!(error.actual_bytes > 8 * 1024);
}
