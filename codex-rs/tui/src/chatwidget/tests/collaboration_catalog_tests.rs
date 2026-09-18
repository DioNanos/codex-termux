//! Server choices are independent of model refresh and restored task settings.

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn collaboration_catalog_selection_clears_restored_prompt_and_survives_model_refresh() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-5.2")).await;
    let restored = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5.2".into(),
            reasoning_effort: Some(ReasoningEffortConfig::Low),
            developer_instructions: Some("stale restored prompt".into()),
        },
    };
    chat.set_effective_collaboration_mode(restored.clone());
    let plan = CollaborationModeMask {
        name: "Server plan".into(),
        mode: Some(ModeKind::Plan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffortConfig::High)),
        developer_instructions: Some(None),
    };
    Arc::make_mut(&mut chat.model_catalog).collaboration_modes = vec![plan.clone()];
    let mut models = chat.model_catalog.try_list_models().unwrap();
    models[0].description = "Refreshed model description".into();
    let request_id = uuid::Uuid::new_v4();
    chat.model_popup_request_id = Some(request_id);
    assert!(chat.on_models_loaded(request_id, Ok(models)));
    assert_eq!(chat.model_catalog.collaboration_modes, vec![plan]);
    assert_eq!(chat.effective_collaboration_mode(), restored);
    assert!(collaboration_modes::default_mode_mask(&chat.model_catalog).is_none());

    chat.cycle_collaboration_mode();
    assert_eq!(
        chat.effective_collaboration_mode(),
        CollaborationMode {
            mode: ModeKind::Plan,
            settings: Settings {
                model: "gpt-5.2".into(),
                reasoning_effort: Some(ReasoningEffortConfig::High),
                developer_instructions: None,
            },
        }
    );
    chat.set_plan_mode_reasoning_effort(Some(ReasoningEffortConfig::Ultra));
    chat.cycle_collaboration_mode();
    assert_eq!(
        chat.effective_reasoning_effort(),
        Some(ReasoningEffortConfig::Ultra)
    );
    chat.set_plan_mode_reasoning_effort(/*effort*/ None);
    assert_eq!(
        chat.effective_reasoning_effort(),
        Some(ReasoningEffortConfig::High)
    );
}

#[tokio::test]
async fn collaboration_catalog_unavailable_preserves_task_and_plan_input() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-5.2")).await;
    Arc::make_mut(&mut chat.model_catalog)
        .collaboration_modes
        .clear();
    for mode in [ModeKind::Default, ModeKind::Plan] {
        let restored = CollaborationMode {
            mode,
            settings: Settings {
                model: "task-model".into(),
                reasoning_effort: Some(ReasoningEffortConfig::Ultra),
                developer_instructions: Some("restored prompt".into()),
            },
        };
        chat.set_effective_collaboration_mode(restored.clone());
        chat.cycle_collaboration_mode();
        assert_eq!(chat.effective_collaboration_mode(), restored);
    }
    chat.set_effective_collaboration_mode(CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "task-model".into(),
            reasoning_effort: Some(ReasoningEffortConfig::Low),
            developer_instructions: None,
        },
    });
    chat.thread_id = Some(ThreadId::new());
    chat.bottom_pane
        .set_composer_text("/plan keep my draft".into(), Vec::new(), Vec::new());
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert_eq!(chat.bottom_pane.composer_text(), "/plan keep my draft");
    assert_eq!(chat.active_mode_kind(), ModeKind::Default);
}
