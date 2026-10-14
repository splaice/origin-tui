use ratatui::buffer::Buffer;
use ratatui::style::Color;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::text::{is_word_char, strip_cols};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (u16, u16),
    pub cursor: (u16, u16),
}

impl Selection {
    pub fn new(anchor: (u16, u16), cursor: (u16, u16)) -> Self {
        Self { anchor, cursor }
    }

    fn ordered(self) -> ((u16, u16), (u16, u16)) {
        let (a, b) = (self.anchor, self.cursor);
        if a.1 < b.1 || (a.1 == b.1 && a.0 <= b.0) {
            (a, b)
        } else {
            (b, a)
        }
    }
}

/// Copy the cells a terminal would select: the first row runs to the right edge,
/// middle rows are full width, and the last row stops at the cursor.
///
/// `continues[y] = Some(indent)` means row `y` is a wrap of the row above. The
/// first `indent` columns are padding, so they are dropped and the rest is joined
/// onto the previous line. That keeps a wrapped clone URL as one string.
pub fn extract(buf: &Buffer, sel: Selection, continues: &[Option<u16>]) -> String {
    let (start, end) = sel.ordered();
    let width = buf.area.width;
    let height = buf.area.height;
    if width == 0 || height == 0 {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut prev_y: Option<u16> = None;
    for y in start.1..=end.1 {
        if y >= height {
            break;
        }
        let (x0, x1) = if start.1 == end.1 {
            (start.0.min(end.0), start.0.max(end.0))
        } else if y == start.1 {
            (start.0, width - 1)
        } else if y == end.1 {
            (0, end.0)
        } else {
            (0, width - 1)
        };
        let x0 = x0.min(width - 1);
        let x1 = x1.min(width - 1);
        let mut piece = row_text(buf, y, x0, x1);
        let cont = continues.get(y as usize).copied().flatten();
        if let (Some(indent), Some(prev)) = (cont, prev_y)
            && prev + 1 == y
            && !lines.is_empty()
        {
            let strip = indent.saturating_sub(x0) as usize;
            piece = strip_cols(&piece, strip);
            piece = piece.trim_end().to_string();
            lines.last_mut().unwrap().push_str(&piece);
            prev_y = Some(y);
            continue;
        }
        lines.push(piece.trim_end().to_string());
        prev_y = Some(y);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        String::new()
    } else {
        text
    }
}

pub fn row_text(buf: &Buffer, y: u16, x0: u16, x1: u16) -> String {
    if y >= buf.area.height || buf.area.width == 0 {
        return String::new();
    }
    let x1 = x1.min(buf.area.width - 1);
    let mut out = String::new();
    let mut x = x0.min(x1);
    while x <= x1 {
        let cell = &buf[(x, y)];
        let sym = cell.symbol();
        let width = UnicodeWidthStr::width(sym).max(1) as u16;
        if !sym.is_empty() {
            out.push_str(sym);
        }
        x = x.saturating_add(width);
        if width > 1 && x > x1 {
            break;
        }
    }
    out
}

pub fn paint(buf: &mut Buffer, sel: Selection, fg: Color, bg: Color) {
    let (start, end) = sel.ordered();
    let width = buf.area.width;
    let height = buf.area.height;
    if width == 0 || height == 0 {
        return;
    }
    for y in start.1..=end.1.min(height - 1) {
        let (x0, x1) = if start.1 == end.1 {
            (start.0.min(end.0), start.0.max(end.0))
        } else if y == start.1 {
            (start.0, width - 1)
        } else if y == end.1 {
            (0, end.0)
        } else {
            (0, width - 1)
        };
        let mut x = x0.min(width - 1);
        let x1 = x1.min(width - 1);
        while x <= x1 {
            let cell = &mut buf[(x, y)];
            cell.set_fg(fg);
            cell.set_bg(bg);
            let sym_width = UnicodeWidthStr::width(cell.symbol()).max(1) as u16;
            x = x.saturating_add(sym_width);
        }
    }
}

pub fn word_span(buf: &Buffer, x: u16, y: u16) -> Option<Selection> {
    if y >= buf.area.height || x >= buf.area.width {
        return None;
    }
    let mut cols: Vec<(u16, char)> = Vec::new();
    let mut cx = 0u16;
    while cx < buf.area.width {
        let sym = buf[(cx, y)].symbol();
        let ch = sym.chars().next().unwrap_or(' ');
        let width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1) as u16;
        cols.push((cx, ch));
        cx = cx.saturating_add(width);
    }
    let idx = cols.iter().rposition(|(col, _)| *col <= x)?;
    let (_, ch) = cols[idx];
    if ch == ' ' || ch == '\u{a0}' {
        return Some(Selection::new((x, y), (x, y)));
    }
    let word = is_word_char(ch);
    let mut lo = idx;
    let mut hi = idx;
    while lo > 0 && same_class(cols[lo - 1].1, word) {
        lo -= 1;
    }
    while hi + 1 < cols.len() && same_class(cols[hi + 1].1, word) {
        hi += 1;
    }
    let start = cols[lo].0;
    let end = cols[hi].0;
    Some(Selection::new((start, y), (end, y)))
}

fn same_class(ch: char, word: bool) -> bool {
    if ch == ' ' {
        return false;
    }
    is_word_char(ch) == word
}

pub fn line_span(buf: &Buffer, y: u16) -> Option<Selection> {
    if y >= buf.area.height || buf.area.width == 0 {
        return None;
    }
    let mut first = None;
    let mut last = None;
    for x in 0..buf.area.width {
        let sym = buf[(x, y)].symbol();
        if sym.trim().is_empty() {
            continue;
        }
        if first.is_none() {
            first = Some(x);
        }
        last = Some(x);
    }
    Some(Selection::new((first?, y), (last?, y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn buf_with(rows: &[&str]) -> Buffer {
        let width = rows
            .iter()
            .map(|r| UnicodeWidthStr::width(*r))
            .max()
            .unwrap_or(0) as u16;
        let mut buf = Buffer::empty(Rect::new(0, 0, width.max(1), rows.len() as u16));
        for (y, row) in rows.iter().enumerate() {
            buf.set_string(0, y as u16, *row, Style::new());
        }
        buf
    }

    #[test]
    fn joins_wrapped_url_across_the_indent() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
        buf.set_string(10, 0, "https://or", Style::new());
        buf.set_string(10, 1, "igin.test/a", Style::new());
        let continues = vec![None, Some(10)];
        let sel = Selection::new((10, 0), (20, 1));
        let text = extract(&buf, sel, &continues);
        assert_eq!(text, "https://origin.test/a");
    }

    #[test]
    fn same_line_is_not_extended_to_the_edge() {
        let buf = buf_with(&["  hello world"]);
        let sel = Selection::new((2, 0), (6, 0));
        assert_eq!(extract(&buf, sel, &[]), "hello");
    }

    #[test]
    fn double_click_word_includes_url_punctuation() {
        let buf = buf_with(&["see https://origin.test/a now"]);
        let sel = word_span(&buf, 8, 0).unwrap();
        assert_eq!(extract(&buf, sel, &[]), "https://origin.test/a");
    }
}
