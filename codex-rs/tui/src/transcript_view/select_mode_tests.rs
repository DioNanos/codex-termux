use super::*;
use crate::history_cell::PlainHistoryCell;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

fn cell(lines: &[&str]) -> Arc<dyn HistoryCell> {
    Arc::new(PlainHistoryCell::new(
        lines
            .iter()
            .map(|line| Line::from((*line).to_owned()))
            .collect(),
    ))
}

fn render(view: &mut TranscriptView, cells: &[Arc<dyn HistoryCell>]) -> Buffer {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 32, /*height*/ 6,
    );
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer, cells);
    buffer
}

fn press(view: &mut TranscriptView, cells: &[Arc<dyn HistoryCell>], code: KeyCode) {
    assert!(matches!(
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE), cells),
        Some(ViewAction::Changed)
    ));
}

#[test]
fn entry_starts_on_the_last_selectable_visible_line() {
    for (cells, expected) in [
        (vec![cell(&["top"]), cell(&["bottom"])], "b"),
        (vec![cell(&["text"]), cell(&[""])], "t"),
        (vec![cell(&["abc", "", "", "", "", "", ""])], "c"),
    ] {
        let mut view = TranscriptView::default();
        render(&mut view, &cells);
        assert!(view.begin_select_mode(&cells));
        press(&mut view, &cells, KeyCode::Char('v'));
        assert_eq!(view.selected_text(&cells).as_deref(), Some(expected));
    }

    let cells = vec![cell(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    view.scroll(&cells, /*rows*/ -2);
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("h"));

    let cells = vec![cell(&["a", "", "", "", "", "", "", "", "", ""])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    assert!(matches!(view.position, Position::Reading(_)));

    let cells = vec![cell(&[""])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(!view.begin_select_mode(&cells));
}

#[test]
fn character_motion_crosses_line_and_entry_boundaries_without_invisible_stops() {
    let cells = vec![cell(&["a", ""]), cell(&[""]), cell(&["", "b"])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    press(&mut view, &cells, KeyCode::Left);
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("a"));

    press(&mut view, &cells, KeyCode::Esc);
    press(&mut view, &cells, KeyCode::Right);
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("b"));

    let cells = vec![cell(&["ab"])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    let separator = Anchor {
        key: EntryKey::cell(&cells[0]),
        index: 0,
        offset: 0,
        row_bias: 1,
    };
    view.selection
        .as_mut()
        .expect("selection")
        .set_keyboard_range(separator, separator);
    press(&mut view, &cells, KeyCode::Right);
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("a"));

    let wrapped = format!("{} one", "1234567890".repeat(3));
    let cells = vec![cell(&[&wrapped])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    press(&mut view, &cells, KeyCode::Left);
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("0"));
}

#[test]
fn vertical_motion_keeps_the_preferred_display_column() {
    let cells = vec![cell(&["abcdef", "x", "abcdef"])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    for _ in 0..3 {
        press(&mut view, &cells, KeyCode::Right);
    }
    press(&mut view, &cells, KeyCode::Up);
    press(&mut view, &cells, KeyCode::Up);
    press(&mut view, &cells, KeyCode::Char('v'));
    assert_eq!(view.selected_text(&cells).as_deref(), Some("d"));
}

#[test]
fn enter_without_a_mark_exits_but_enter_with_a_mark_copies_and_follows() {
    let cells = vec![cell(&["copy me"])];
    let mut view = TranscriptView::default();
    render(&mut view, &cells);
    assert!(view.begin_select_mode(&cells));
    assert!(matches!(
        view.handle_key(KeyCode::Enter.into(), &cells),
        Some(ViewAction::ExitSelectMode)
    ));

    assert!(view.begin_select_mode(&cells));
    press(&mut view, &cells, KeyCode::Char('v'));
    assert!(matches!(
        view.handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('v'), KeyModifiers::NONE, KeyEventKind::Repeat,),
            &cells,
        ),
        Some(ViewAction::Changed)
    ));
    assert!(matches!(
        view.handle_key(KeyCode::Enter.into(), &cells),
        Some(ViewAction::CopyAndFollow(text)) if text == "c"
    ));
}
