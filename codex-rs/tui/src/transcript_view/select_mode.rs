//! Keyboard-owned transcript cursor and mark state over the existing selection snapshot.
//!
//! An empty `Selection` is the caret. Marking only records its fixed and active anchors here;
//! copying, retained revisions, serialization, and asynchronous completion remain owned by
//! `Selection`.

use super::*;
use crate::text_selection::SelectionUnit;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

mod motion;

#[derive(Clone, Copy, Default)]
pub(super) struct SelectMode {
    pub(super) mark: Option<Mark>,
    pub(super) preferred_column: Option<u16>,
}

#[derive(Clone, Copy)]
pub(super) struct Mark {
    anchor: Anchor,
    active: Anchor,
}

#[derive(Clone, Copy)]
enum CursorMotion {
    Character(KeyCode),
    Rows(isize),
}

impl SelectMode {
    pub(super) fn cursor(self, selection: &Selection) -> Anchor {
        self.mark.map_or(selection.end, |mark| mark.active)
    }
}

impl TranscriptView {
    /// Enter at the bottommost visible selectable position.
    #[cfg_attr(not(test), expect(dead_code, reason = "used by a later stack layer"))]
    pub(crate) fn begin_select_mode(&mut self, cells: &[Arc<dyn HistoryCell>]) -> bool {
        if self.select_mode.is_some() {
            return true;
        }
        self.copy_mode = None;
        self.cancel_beginning();
        if self.selection.is_none() {
            let cursor = self
                .visible
                .iter()
                .rev()
                .find_map(|visible| {
                    let offset = visible.layout.first_cursor_position(visible.row)?;
                    Some(Anchor {
                        key: visible.key,
                        index: visible.index,
                        offset,
                        row_bias: visible.layout.row_for_offset(offset) as isize
                            - visible.row as isize,
                    })
                })
                .or_else(|| self.select_boundary(cells, /*end*/ true));
            let Some(cursor) = cursor else {
                return false;
            };
            let snapshot = self.capture_snapshot(cells);
            self.selection = Some(Selection::keyboard(snapshot, cursor));
        }
        if self.position == Position::Latest {
            self.hold_position();
        }
        self.select_mode = Some(SelectMode::default());
        self.reveal_select_cursor(cells);
        true
    }

    /// Leave the mode while retaining its displayed revision at the paused reading position.
    pub(crate) fn end_select_mode(&mut self) {
        if self.select_mode.take().is_none() {
            return;
        }
        self.cancel_beginning();
        if self.position == Position::Latest {
            self.hold_position();
        }
        if let Some(selection) = self.selection.take() {
            self.held_reading = Some(selection.snapshot);
        }
    }

    pub(crate) fn handle_select_mode_key(
        &mut self,
        key: KeyEvent,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Option<ViewAction> {
        if self.select_mode.is_none() || key.kind == KeyEventKind::Release {
            return None;
        }
        let (code, modifiers) = crate::key_hint::normalize_key_parts(key.code, key.modifiers);
        let pressed = key.kind == KeyEventKind::Press;
        if crate::text_selection::is_copy_key(key)
            || (code == KeyCode::Char('y') && modifiers == KeyModifiers::NONE)
            || (code == KeyCode::Enter
                && modifiers == KeyModifiers::NONE
                && key.kind == KeyEventKind::Press)
        {
            let Some(text) = self.selected_text(cells).filter(|text| !text.is_empty()) else {
                if code == KeyCode::Enter {
                    return Some(self.exit_select_mode());
                }
                return Some(ViewAction::Changed);
            };
            return Some(if code == KeyCode::Enter {
                ViewAction::CopyAndFollow(text)
            } else {
                ViewAction::Copy(text)
            });
        }
        if pressed && code == KeyCode::Esc && modifiers == KeyModifiers::NONE {
            if self.select_mode.is_some_and(|mode| mode.mark.is_some()) {
                self.clear_select_mark();
                return Some(ViewAction::Changed);
            }
            return Some(self.exit_select_mode());
        }
        if pressed && code == KeyCode::Char('q') && modifiers == KeyModifiers::NONE {
            return Some(self.exit_select_mode());
        }
        if pressed
            && matches!(code, KeyCode::Char('v') | KeyCode::Char(' '))
            && modifiers == KeyModifiers::NONE
        {
            self.start_select_mark(cells);
            return Some(ViewAction::Changed);
        }

        let shift_arrow = modifiers == KeyModifiers::SHIFT
            && matches!(
                code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            );
        if shift_arrow && self.select_mode.is_some_and(|mode| mode.mark.is_none()) {
            self.start_select_mark(cells);
        }
        let motion = match (code, modifiers) {
            (
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down,
                KeyModifiers::NONE | KeyModifiers::SHIFT,
            ) => {
                if matches!(code, KeyCode::Up | KeyCode::Down) {
                    CursorMotion::Rows(if code == KeyCode::Up { -1 } else { 1 })
                } else {
                    CursorMotion::Character(code)
                }
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) => CursorMotion::Character(KeyCode::Left),
            (KeyCode::Char('l'), KeyModifiers::NONE) => CursorMotion::Character(KeyCode::Right),
            (KeyCode::Char('k'), KeyModifiers::NONE) => CursorMotion::Rows(-1),
            (KeyCode::Char('j'), KeyModifiers::NONE) => CursorMotion::Rows(1),
            _ => return None,
        };
        self.move_select_cursor(cells, motion);
        Some(ViewAction::Changed)
    }

    fn exit_select_mode(&mut self) -> ViewAction {
        self.end_select_mode();
        ViewAction::ExitSelectMode
    }

    fn start_select_mark(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        let Some(mut mode) = self.select_mode else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let cursor = mode.cursor(selection);
        mode.mark = match mode.mark {
            Some(_) => {
                self.clear_select_mark();
                return;
            }
            None => Some(Mark {
                anchor: cursor,
                active: cursor,
            }),
        };
        mode.preferred_column = None;
        self.select_mode = Some(mode);
        self.sync_select_range(cells);
    }

    fn clear_select_mark(&mut self) {
        let Some(mode) = &mut self.select_mode else {
            return;
        };
        let Some(selection) = &mut self.selection else {
            return;
        };
        let cursor = mode.cursor(selection);
        mode.mark = None;
        mode.preferred_column = None;
        selection.set_keyboard_range(cursor, cursor);
    }

    fn reveal_select_cursor(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        let Some(mode) = self.select_mode else {
            return;
        };
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let cursor = mode.cursor(selection);
        let index = self.resolve(cells, cursor);
        let row = self.layout(cells, index).map_or(/*default*/ 0, |layout| {
            layout
                .row_for_offset(cursor.offset)
                .saturating_add_signed(cursor.row_bias.saturating_neg())
        });
        if !self
            .visible
            .iter()
            .any(|visible| visible.index == index && visible.row == row)
        {
            self.position = Position::Reading(Anchor { index, ..cursor });
        }
    }

    pub(super) fn render_select_cursor(&self, buf: &mut Buffer) {
        let (Some(mode), Some(selection)) = (self.select_mode, self.selection.as_ref()) else {
            return;
        };
        let cursor = mode.cursor(selection);
        let Some((y, visible)) = self.visible.iter().enumerate().find(|(_, visible)| {
            visible.key == cursor.key
                && visible.row
                    == visible
                        .layout
                        .row_for_offset(cursor.offset)
                        .saturating_add_signed(cursor.row_bias.saturating_neg())
        }) else {
            return;
        };
        let column = visible
            .layout
            .column_for_offset_on_row(visible.row, cursor.offset);
        if column < self.area.width {
            let area = Rect::new(
                self.area.x + column,
                self.area.y + y as u16,
                /*width*/ 1,
                /*height*/ 1,
            );
            buf.set_style(area, crate::style::selection_style().underlined());
        }
    }
}

#[cfg(test)]
#[path = "select_mode_tests.rs"]
mod tests;
