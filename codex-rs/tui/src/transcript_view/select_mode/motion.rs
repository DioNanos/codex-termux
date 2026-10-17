//! Basic cursor motions and marked-range derivation for keyboard transcript selection.

use super::*;
use crossterm::event::KeyCode;
use unicode_segmentation::UnicodeSegmentation;

impl TranscriptView {
    pub(super) fn move_select_cursor(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        motion: CursorMotion,
    ) {
        let Some(mut mode) = self.select_mode else {
            return;
        };
        self.cancel_beginning();
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let cursor = mode.cursor(selection);
        let snapshot = self.snapshot_cells();
        let cells = snapshot.as_deref().unwrap_or(cells);
        let next = match motion {
            CursorMotion::Character(code) => {
                mode.preferred_column = None;
                self.move_select_character(cells, cursor, code)
            }
            CursorMotion::Rows(rows) => {
                let index = self.resolve(cells, cursor);
                let layout = self.layout(cells, index);
                let column = mode.preferred_column.or_else(|| {
                    layout.as_ref().map(|layout| {
                        let row = layout
                            .row_for_offset(cursor.offset)
                            .saturating_add_signed(cursor.row_bias.saturating_neg());
                        layout.column_for_offset_on_row(row, cursor.offset)
                    })
                });
                mode.preferred_column = column;
                self.move_select_rows(cells, cursor, rows, column)
            }
        };
        let Some(next) = next else {
            self.select_mode = Some(mode);
            return;
        };
        if let Some(mark) = &mut mode.mark {
            mark.active = next;
        } else if let Some(selection) = &mut self.selection {
            selection.set_keyboard_range(next, next);
        }
        self.select_mode = Some(mode);
        self.sync_select_range(cells);
        self.reveal_select_cursor(cells);
    }

    fn move_select_character(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        cursor: Anchor,
        code: KeyCode,
    ) -> Option<Anchor> {
        let mut index = self.resolve(cells, cursor);
        let layout = self.layout(cells, index)?;
        let offset = layout
            .text()
            .floor_char_boundary(cursor.offset.min(layout.text().len()));
        let local = match code {
            KeyCode::Left if offset > 0 => selectable_graphemes(&layout.text()[..offset])
                .rfind(|(offset, _)| layout.is_cursor_position(*offset))
                .map(|(offset, _)| offset),
            KeyCode::Right if offset < layout.text().len() => {
                selectable_graphemes(&layout.text()[offset..])
                    .filter(|(next, _)| layout.is_cursor_position(offset + *next))
                    .find(|(next, _)| cursor.row_bias != 0 || *next > 0)
                    .map(|(next, _)| offset + next)
            }
            _ => None,
        };
        if let Some(offset) = local {
            return Some(Anchor {
                key: self.entry_key(cells, index),
                index,
                offset,
                row_bias: 0,
            });
        }
        loop {
            index = match code {
                KeyCode::Left => index.checked_sub(1)?,
                KeyCode::Right => index + 1,
                _ => return None,
            };
            let next = self.layout(cells, index)?;
            let offset = match code {
                KeyCode::Left => selectable_graphemes(next.text())
                    .rfind(|(offset, _)| next.is_cursor_position(*offset))
                    .map(|(offset, _)| offset),
                KeyCode::Right => selectable_graphemes(next.text())
                    .find(|(offset, _)| next.is_cursor_position(*offset))
                    .map(|(offset, _)| offset),
                _ => None,
            };
            if let Some(offset) = offset {
                return Some(Anchor {
                    key: self.entry_key(cells, index),
                    index,
                    offset,
                    row_bias: 0,
                });
            }
        }
    }

    fn move_select_rows(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        mut cursor: Anchor,
        rows: isize,
        column: Option<u16>,
    ) -> Option<Anchor> {
        let code = if rows < 0 { KeyCode::Up } else { KeyCode::Down };
        for _ in 0..rows.unsigned_abs() {
            let Some(mut next) = self.move_selection_endpoint(cells, cursor, code, column) else {
                break;
            };
            let index = self.resolve(cells, next);
            if let Some(layout) = self.layout(cells, index) {
                let row = layout
                    .row_for_offset(next.offset)
                    .saturating_add_signed(next.row_bias.saturating_neg());
                next.offset = layout.cursor_position_at(row, column.unwrap_or_default());
                next.row_bias = layout.row_for_offset(next.offset) as isize - row as isize;
            }
            if next == cursor {
                break;
            }
            cursor = next;
        }
        Some(cursor)
    }

    pub(super) fn select_boundary(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        end: bool,
    ) -> Option<Anchor> {
        let count = cells.len() + usize::from(self.layout(cells, cells.len()).is_some());
        let mut index = if end { count.checked_sub(1)? } else { 0 };
        loop {
            let layout = self.layout(cells, index)?;
            let offset = if end {
                selectable_graphemes(layout.text())
                    .rfind(|(offset, _)| layout.is_cursor_position(*offset))
                    .map(|(offset, _)| offset)
            } else {
                selectable_graphemes(layout.text())
                    .find(|(offset, _)| layout.is_cursor_position(*offset))
                    .map(|(offset, _)| offset)
            };
            if let Some(offset) = offset {
                return Some(Anchor {
                    key: self.entry_key(cells, index),
                    index,
                    offset,
                    row_bias: 0,
                });
            }
            if end {
                index = index.checked_sub(1)?;
            } else {
                index += 1;
                if index >= count {
                    return None;
                }
            }
        }
    }

    pub(super) fn sync_select_range(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        let Some(mark) = self.select_mode.and_then(|mode| mode.mark) else {
            return;
        };
        let snapshot = self.snapshot_cells();
        let cells = snapshot.as_deref().unwrap_or(cells);
        let after_anchor = self.after_grapheme(cells, mark.anchor);
        let after_active = self.after_grapheme(cells, mark.active);
        let Some(mut selection) = self.selection.take() else {
            return;
        };
        let (start, end) =
            if self.anchor_order(cells, mark.anchor) <= self.anchor_order(cells, mark.active) {
                (mark.anchor, after_active)
            } else {
                (after_anchor, mark.active)
            };
        selection.set_keyboard_marked_range(
            start,
            end,
            (mark.anchor, after_anchor),
            SelectionUnit::Character,
        );
        self.pin_selection_range(cells, &mut selection);
        self.selection = Some(selection);
    }

    fn after_grapheme(&mut self, cells: &[Arc<dyn HistoryCell>], anchor: Anchor) -> Anchor {
        let index = self.resolve(cells, anchor);
        let Some(layout) = self.layout(cells, index) else {
            return anchor;
        };
        let offset = layout
            .text()
            .floor_char_boundary(anchor.offset.min(layout.text().len()));
        let row = layout
            .row_for_offset(offset)
            .saturating_add_signed(anchor.row_bias.saturating_neg());
        let offset = layout.after_cursor_grapheme(row, offset);
        Anchor {
            key: self.entry_key(cells, index),
            index,
            offset,
            row_bias: 0,
        }
    }

    fn anchor_order(&self, cells: &[Arc<dyn HistoryCell>], anchor: Anchor) -> (usize, usize) {
        (self.resolve(cells, anchor), anchor.offset)
    }
}

fn selectable_graphemes(text: &str) -> impl DoubleEndedIterator<Item = (usize, &str)> {
    text.grapheme_indices(/*is_extended*/ true)
        .filter(|(_, grapheme)| !grapheme.contains('\n'))
}
