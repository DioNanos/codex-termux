//! Regression coverage for transcript viewer input and prompt selection.
//!
//! The default-off feature must leave the existing viewer and its draft intact.

use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

async fn press_key(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    app_server: &mut AppServerSession,
    code: KeyCode,
) -> Result<()> {
    app.handle_tui_event(
        tui,
        app_server,
        TuiEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn transcript_flag_off_preserves_viewer_and_backtracking() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let keymap_config = toml::from_str("[composer]\nsubmit = [\"ctrl-x enter\"]")?;
    app.keymap =
        crate::keymap::RuntimeKeymap::from_config(&keymap_config).expect("valid composer chord");
    app.chat_widget
        .apply_keymap_update(keymap_config, &app.keymap);
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells = ["first", "second"]
        .map(|message| {
            Arc::new(UserHistoryCell {
                message: message.into(),
                text_elements: Vec::new(),
                local_image_paths: Vec::new(),
                remote_image_urls: Vec::new(),
            }) as Arc<dyn HistoryCell>
        })
        .to_vec();
    app.chat_widget
        .apply_external_edit("preserved draft".into());
    app.open_transcript_overlay(&mut tui);
    for event in [
        TuiEvent::Paste("not composer input".into()),
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
    ] {
        app.handle_tui_event(&mut tui, &mut app_server, event)
            .await?;
    }
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "preserved draft"
    );
    let chord_prefix = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Key(chord_prefix))
        .await?;
    assert!(!app.key_chord_matcher.is_pending());
    assert!(!app.backtrack.overlay_preview_active);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let Some(Overlay::Transcript(overlay)) = &mut app.overlay else {
        panic!("viewer closed")
    };
    overlay.render(area, &mut buffer);
    let text = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("transcript_flag_off_viewer", text);
    for (key, selected) in [
        (KeyCode::Esc, 1),
        (KeyCode::Esc, 0),
        (KeyCode::Right, 1),
        (KeyCode::Right, 1),
    ] {
        press_key(&mut app, &mut tui, &mut app_server, key).await?;
        assert_eq!(app.backtrack.nth_user_message, selected);
    }
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Enter).await?;
    assert!(app.overlay.is_none());
    assert!(
        std::iter::from_fn(|| app_event_rx.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::ForkSessionForPromptEdit {
                nth_user_message: 1,
                ..
            }
        ))
    );
    Ok(())
}
