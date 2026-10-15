use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Body,
    Strong,
    Emphasis,
    StrongEmphasis,
    Code,
    CodeBlock,
    H1,
    H2,
    H3,
    Link,
    Dim,
    Strike,
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub pieces: Vec<Piece>,
    pub band: bool,
}

pub fn render(src: &str, width: usize) -> Vec<Row> {
    if width == 0 {
        return Vec::new();
    }
    let src = src.trim_start_matches('\u{feff}');
    if src.trim().is_empty() {
        return vec![note("This README is empty.")];
    }
    let rows = render_blocks(src, width, false);
    if rows.iter().all(is_blank) {
        vec![note("This README is empty.")]
    } else {
        rows
    }
}

fn render_blocks(src: &str, width: usize, tight: bool) -> Vec<Row> {
    if width == 0 {
        return Vec::new();
    }
    let lines = lines_of(src);
    let mut rows = Vec::new();
    let mut i = 0;
    let mut separated = false;
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            if !rows.is_empty() {
                separated = true;
            }
            i += 1;
            continue;
        }
        if let Some(next) = skip_comment(&lines, i) {
            i = next;
            continue;
        }
        if open_fence(lines[i]).is_some() {
            begin_block(&mut rows, &mut separated, tight);
            i = emit_fence(&mut rows, &lines, i, width);
            continue;
        }
        if let Some((level, text)) = atx(lines[i]) {
            if text.is_empty() {
                i += 1;
                continue;
            }
            begin_block(&mut rows, &mut separated, tight);
            emit_heading(&mut rows, level, text, width);
            i += 1;
            continue;
        }
        if is_rule(lines[i]) {
            begin_block(&mut rows, &mut separated, tight);
            rows.push(rule_row(width));
            i += 1;
            continue;
        }
        if is_quote(lines[i]) {
            begin_block(&mut rows, &mut separated, tight);
            i = emit_quote(&mut rows, &lines, i, width);
            continue;
        }
        if marker_at(lines[i]).is_some_and(|marker| marker.indent <= 3) {
            begin_block(&mut rows, &mut separated, tight);
            i = emit_list(&mut rows, &lines, i, width);
            continue;
        }
        if is_table(&lines, i) {
            begin_block(&mut rows, &mut separated, tight);
            i = emit_table(&mut rows, &lines, i, width);
            continue;
        }
        if is_indented_code(lines[i]) {
            begin_block(&mut rows, &mut separated, tight);
            i = emit_indented(&mut rows, &lines, i, width);
            continue;
        }
        begin_block(&mut rows, &mut separated, tight);
        i = emit_paragraph(&mut rows, &lines, i, width);
    }
    while rows.last().is_some_and(is_blank) {
        rows.pop();
    }
    rows
}

fn begin_block(rows: &mut Vec<Row>, separated: &mut bool, tight: bool) {
    let has_content = rows.last().is_some_and(|row| !is_blank(row));
    if has_content && (!tight || *separated) {
        rows.push(blank());
    }
    *separated = false;
}

fn lines_of(src: &str) -> Vec<&str> {
    src.split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect()
}

fn note(text: &str) -> Row {
    Row {
        pieces: vec![Piece {
            text: text.to_string(),
            mark: Mark::Dim,
        }],
        band: false,
    }
}

fn blank() -> Row {
    Row {
        pieces: Vec::new(),
        band: false,
    }
}

fn is_blank(row: &Row) -> bool {
    row.pieces.iter().all(|piece| piece.text.trim().is_empty())
}

fn rule_row(width: usize) -> Row {
    Row {
        pieces: vec![Piece {
            text: "─".repeat(width.max(1)),
            mark: Mark::Rule,
        }],
        band: false,
    }
}

fn skip_comment(lines: &[&str], start: usize) -> Option<usize> {
    if !lines[start].trim_start().starts_with("<!--") {
        return None;
    }
    let mut i = start;
    loop {
        if lines[i].contains("-->") {
            return Some(i + 1);
        }
        i += 1;
        if i >= lines.len() {
            return Some(i);
        }
    }
}

fn open_fence(line: &str) -> Option<(char, usize, usize)> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = rest.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = rest.chars().take_while(|ch| *ch == marker).count();
    if count < 3 {
        return None;
    }
    let info = &rest[count..];
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((marker, count, indent))
}

fn is_close_fence(line: &str, marker: char, count: usize) -> bool {
    let indent = leading_spaces(line);
    if indent > 3 {
        return false;
    }
    let rest = line[indent..].trim_end();
    rest.len() >= count && rest.chars().all(|ch| ch == marker)
}

fn emit_fence(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let Some((marker, count, indent)) = open_fence(lines[start]) else {
        return start + 1;
    };
    let mut end = start + 1;
    while end < lines.len() && !is_close_fence(lines[end], marker, count) {
        end += 1;
    }
    let content: Vec<String> = lines[start + 1..end]
        .iter()
        .map(|line| expand_tabs(strip_spaces(line, indent)))
        .collect();
    emit_code(rows, &content, width);
    if end < lines.len() { end + 1 } else { end }
}

fn emit_indented(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let mut i = start;
    let mut last = start;
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        if is_indented_code(lines[i]) {
            last = i;
            i += 1;
            continue;
        }
        break;
    }
    let content: Vec<String> = lines[start..=last]
        .iter()
        .map(|line| {
            if let Some(stripped) = line.strip_prefix('\t') {
                expand_tabs(stripped)
            } else {
                expand_tabs(strip_spaces(line, 4))
            }
        })
        .collect();
    emit_code(rows, &content, width);
    last + 1
}

fn is_indented_code(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

fn emit_code(rows: &mut Vec<Row>, lines: &[String], width: usize) {
    let width = width.max(1);
    for line in lines {
        if line.is_empty() {
            rows.push(Row {
                pieces: Vec::new(),
                band: true,
            });
            continue;
        }
        let mut rest = line.as_str();
        loop {
            let (head, tail) = split_width(rest, width);
            if head.is_empty() {
                break;
            }
            rows.push(Row {
                pieces: vec![Piece {
                    text: head.to_string(),
                    mark: Mark::CodeBlock,
                }],
                band: true,
            });
            if tail.is_empty() {
                break;
            }
            rest = tail;
        }
    }
}

fn atx(line: &str) -> Option<(u8, &str)> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let level = rest.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    Some((level as u8, strip_atx_close(after.trim())))
}

fn strip_atx_close(text: &str) -> &str {
    let trimmed = text.trim_end_matches('#').trim_end();
    if trimmed.len() < text.len()
        && (trimmed.is_empty() || text.as_bytes().get(trimmed.len()) == Some(&b' '))
    {
        trimmed
    } else {
        text
    }
}

fn emit_heading(rows: &mut Vec<Row>, level: u8, text: &str, width: usize) {
    let mark = match level {
        1 => Mark::H1,
        2 => Mark::H2,
        _ => Mark::H3,
    };
    let mut pieces = inline(text);
    for piece in &mut pieces {
        if !matches!(piece.mark, Mark::Code | Mark::Link) {
            piece.mark = mark;
        }
    }
    rows.extend(wrap_pieces(&pieces, width, "", ""));
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let mut kind = None;
    let mut count = 0;
    for ch in trimmed.chars() {
        if ch == ' ' || ch == '\t' {
            continue;
        }
        if !matches!(ch, '-' | '_' | '*') {
            return false;
        }
        if kind.is_some() && kind != Some(ch) {
            return false;
        }
        kind = Some(ch);
        count += 1;
    }
    count >= 3
}

fn setext_level(line: &str) -> Option<u8> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().all(|ch| ch == '=') {
        return Some(1);
    }
    if trimmed.chars().all(|ch| ch == '-') {
        return Some(2);
    }
    None
}

fn is_quote(line: &str) -> bool {
    strip_quote(line).is_some()
}

fn strip_quote(line: &str) -> Option<&str> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = line[indent..].strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn emit_quote(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let mut i = start;
    let mut inner = Vec::new();
    while i < lines.len() {
        if let Some(content) = strip_quote(lines[i]) {
            inner.push(content);
            i += 1;
        } else {
            break;
        }
    }
    let src = inner.join("\n");
    let rendered = if src.trim().is_empty() {
        vec![blank()]
    } else {
        render_blocks(&src, width.saturating_sub(2).max(1), false)
    };
    for row in rendered {
        rows.push(prefix_row("│ ", Mark::Rule, dim_body(row)));
    }
    i
}

fn dim_body(mut row: Row) -> Row {
    for piece in &mut row.pieces {
        if piece.mark == Mark::Body {
            piece.mark = Mark::Dim;
        }
    }
    row
}

struct Marker<'a> {
    indent: usize,
    kind: ItemKind,
    content: &'a str,
    number: u64,
}

#[derive(Clone, Copy)]
enum ItemKind {
    Bullet,
    Ordered,
    Task { done: bool },
}

fn marker_at(line: &str) -> Option<Marker<'_>> {
    let indent = leading_spaces(line);
    let rest = &line[indent..];
    if rest.is_empty() {
        return None;
    }
    if let Some((done, content)) = task_content(rest) {
        return Some(Marker {
            indent,
            kind: ItemKind::Task { done },
            content,
            number: 0,
        });
    }
    let bytes = rest.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && digits < 9 && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0
        && digits < bytes.len()
        && (bytes[digits] == b'.' || bytes[digits] == b')')
        && rest[digits + 1..].starts_with(' ')
    {
        let number = rest[..digits].parse().unwrap_or(1);
        return Some(Marker {
            indent,
            kind: ItemKind::Ordered,
            content: &rest[digits + 2..],
            number,
        });
    }
    if let Some(content) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        return Some(Marker {
            indent,
            kind: ItemKind::Bullet,
            content,
            number: 0,
        });
    }
    None
}

fn task_content(rest: &str) -> Option<(bool, &str)> {
    const MARKS: &[(&str, bool)] = &[
        ("- [x]", true),
        ("- [X]", true),
        ("- [ ]", false),
        ("* [x]", true),
        ("* [X]", true),
        ("* [ ]", false),
        ("+ [x]", true),
        ("+ [X]", true),
        ("+ [ ]", false),
    ];
    for (prefix, done) in MARKS {
        let Some(after) = rest.strip_prefix(prefix) else {
            continue;
        };
        if after.is_empty() {
            return Some((*done, ""));
        }
        if let Some(content) = after.strip_prefix(' ') {
            return Some((*done, content));
        }
    }
    None
}

fn emit_list(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let base = marker_at(lines[start])
        .map(|marker| marker.indent)
        .unwrap_or(0);
    let mut i = start;
    while i < lines.len() {
        let Some(marker) = marker_at(lines[i]) else {
            break;
        };
        if marker.indent != base {
            break;
        }
        let mut body = vec![marker.content];
        let number = marker.number;
        let kind = marker.kind;
        i += 1;
        while i < lines.len() {
            if lines[i].trim().is_empty() {
                let mut k = i + 1;
                while k < lines.len() && lines[k].trim().is_empty() {
                    k += 1;
                }
                if k >= lines.len() {
                    break;
                }
                let sibling = marker_at(lines[k]).is_some_and(|next| next.indent <= base);
                let dedent = leading_spaces(lines[k]) <= base && marker_at(lines[k]).is_none();
                if sibling || dedent {
                    break;
                }
                body.push(lines[i]);
                i += 1;
                continue;
            }
            if marker_at(lines[i]).is_some_and(|next| next.indent <= base) {
                break;
            }
            if leading_spaces(lines[i]) <= base && marker_at(lines[i]).is_none() {
                break;
            }
            body.push(lines[i]);
            i += 1;
        }
        emit_item(rows, &body, kind, number, width);
    }
    i
}

fn emit_item(rows: &mut Vec<Row>, body: &[&str], kind: ItemKind, number: u64, width: usize) {
    let prefix = match kind {
        ItemKind::Bullet => "• ".to_string(),
        ItemKind::Ordered => format!("{number}. "),
        ItemKind::Task { done: true } => "☑ ".to_string(),
        ItemKind::Task { done: false } => "☐ ".to_string(),
    };
    let hang = " ".repeat(display_width(&prefix));
    let src = item_source(body);
    let inner_width = width.saturating_sub(display_width(&hang)).max(1);
    let inner = if src.trim().is_empty() {
        Vec::new()
    } else {
        render_blocks(&src, inner_width, true)
    };
    if inner.is_empty() {
        rows.push(Row {
            pieces: vec![Piece {
                text: prefix,
                mark: Mark::Body,
            }],
            band: false,
        });
        return;
    }
    for (index, row) in inner.into_iter().enumerate() {
        let pad = if index == 0 {
            prefix.as_str()
        } else {
            hang.as_str()
        };
        rows.push(prefix_row(pad, Mark::Body, row));
    }
}

fn item_source(body: &[&str]) -> String {
    if body.is_empty() {
        return String::new();
    }
    let rest = &body[1..];
    let min = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| leading_spaces(line))
        .min();
    let mut lines = vec![body[0].trim_end().to_string()];
    for line in rest {
        if line.trim().is_empty() {
            lines.push(String::new());
        } else if let Some(min) = min {
            lines.push(strip_spaces(line, min).to_string());
        } else {
            lines.push((*line).to_string());
        }
    }
    lines.join("\n")
}

fn prefix_row(prefix: &str, mark: Mark, row: Row) -> Row {
    if prefix.is_empty() {
        return row;
    }
    let mut pieces = vec![Piece {
        text: prefix.to_string(),
        mark,
    }];
    pieces.extend(row.pieces);
    Row {
        pieces,
        band: row.band,
    }
}

fn is_table(lines: &[&str], index: usize) -> bool {
    let Some(header) = split_cells(lines.get(index).copied().unwrap_or("")) else {
        return false;
    };
    let Some(sep) = split_cells(lines.get(index + 1).copied().unwrap_or("")) else {
        return false;
    };
    !header.is_empty() && header.len() == sep.len() && sep.iter().all(|cell| is_sep_cell(cell))
}

fn split_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    Some(
        trimmed
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect(),
    )
}

fn is_sep_cell(cell: &str) -> bool {
    let core = cell.trim().trim_matches(':');
    !core.is_empty() && core.chars().all(|ch| ch == '-')
}

fn emit_table(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let header = split_cells(lines[start]).unwrap_or_default();
    let mut body = Vec::new();
    let mut i = start + 2;
    while i < lines.len() && !lines[i].trim().is_empty() {
        let Some(cells) = split_cells(lines[i]) else {
            break;
        };
        body.push(cells);
        i += 1;
    }
    let cols = header
        .len()
        .max(body.iter().map(Vec::len).max().unwrap_or(0));
    if cols == 0 {
        return i;
    }
    let mut header_cells: Vec<Vec<Piece>> = (0..cols)
        .map(|col| {
            let mut pieces = inline(header.get(col).map(String::as_str).unwrap_or(""));
            for piece in &mut pieces {
                if piece.mark == Mark::Body {
                    piece.mark = Mark::Strong;
                }
            }
            pieces
        })
        .collect();
    let mut body_cells: Vec<Vec<Vec<Piece>>> = body
        .iter()
        .map(|row| {
            (0..cols)
                .map(|col| inline(row.get(col).map(String::as_str).unwrap_or("")))
                .collect()
        })
        .collect();
    let mut widths = vec![0usize; cols];
    for (col, cell) in header_cells.iter().enumerate() {
        widths[col] = pieces_width(cell);
    }
    for row in &body_cells {
        for (col, cell) in row.iter().enumerate() {
            widths[col] = widths[col].max(pieces_width(cell));
        }
    }
    let gap = 3usize;
    let gaps = gap * cols.saturating_sub(1);
    let natural = widths.iter().sum::<usize>() + gaps;
    if natural > width && width > gaps + cols {
        let budget = width - gaps;
        let content: usize = widths.iter().sum();
        for column in &mut widths {
            *column = column
                .saturating_mul(budget)
                .checked_div(content)
                .unwrap_or(1)
                .max(1);
        }
    }
    rows.push(table_line(&mut header_cells, &widths));
    for row in &mut body_cells {
        rows.push(table_line(row, &widths));
    }
    i
}

fn table_line(cells: &mut [Vec<Piece>], widths: &[usize]) -> Row {
    let mut pieces = Vec::new();
    for (index, cell) in cells.iter_mut().enumerate() {
        if index > 0 {
            pieces.push(Piece {
                text: " │ ".to_string(),
                mark: Mark::Dim,
            });
        }
        pieces.extend(fit_pieces(cell, widths.get(index).copied().unwrap_or(0)));
    }
    Row {
        pieces,
        band: false,
    }
}

fn fit_pieces(pieces: &[Piece], width: usize) -> Vec<Piece> {
    if width == 0 {
        return Vec::new();
    }
    if pieces_width(pieces) <= width {
        let mut out = pieces.to_vec();
        let pad = width - pieces_width(&out);
        if pad > 0 {
            let mark = out.last().map(|piece| piece.mark).unwrap_or(Mark::Body);
            out.push(Piece {
                text: " ".repeat(pad),
                mark,
            });
        }
        return out;
    }
    let mut out = Vec::new();
    let mut used = 0usize;
    for piece in pieces {
        if used >= width.saturating_sub(1) {
            break;
        }
        let mut text = String::new();
        for ch in piece.text.chars() {
            let next = used + text_width(&text) + char_width(ch);
            if next > width.saturating_sub(1) {
                break;
            }
            text.push(ch);
        }
        if text.is_empty() {
            continue;
        }
        used += text_width(&text);
        out.push(Piece {
            text,
            mark: piece.mark,
        });
    }
    out.push(Piece {
        text: "…".to_string(),
        mark: Mark::Dim,
    });
    out
}

fn emit_paragraph(rows: &mut Vec<Row>, lines: &[&str], start: usize, width: usize) -> usize {
    let mut i = start + 1;
    let mut collected = vec![lines[start]];
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty()
            || atx(line).is_some()
            || open_fence(line).is_some()
            || is_quote(line)
            || is_table(lines, i)
            || marker_at(line).is_some_and(|marker| marker.indent <= 3)
        {
            break;
        }
        if is_rule(line) && setext_level(line).is_none() {
            break;
        }
        if let Some(level) = setext_level(line) {
            emit_heading(rows, level, &join_lines(&collected), width);
            return i + 1;
        }
        collected.push(line);
        i += 1;
    }
    let pieces = inline(&join_lines(&collected));
    rows.extend(wrap_pieces(&pieces, width, "", ""));
    i
}

fn join_lines(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut hard = false;
    for (index, line) in lines.iter().enumerate() {
        let next_hard = line.ends_with("  ") || line.ends_with('\\');
        let text = line.strip_suffix('\\').unwrap_or(line).trim();
        if index > 0 {
            out.push(if hard { '\n' } else { ' ' });
        }
        out.push_str(text);
        hard = next_hard;
    }
    out
}

fn inline(src: &str) -> Vec<Piece> {
    let tokens = tokenize(src);
    let matched = match_delims(&tokens);
    merge_pieces(emit_tokens(&tokens, &matched))
}

fn tokenize(src: &str) -> Vec<Tok> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < src.len() {
        let Some(ch) = next_char(src, i) else {
            break;
        };
        if ch == '\n' {
            flush_text(&mut tokens, &mut buf);
            tokens.push(Tok::Break);
            i += 1;
            continue;
        }
        if ch == '\\'
            && let Some(escaped) = next_char(src, i + 1)
            && escaped.is_ascii_punctuation()
        {
            buf.push(escaped);
            i = i + 1 + escaped.len_utf8();
            continue;
        }
        if ch == '`'
            && let Some((content, end)) = code_span(src, i)
        {
            flush_text(&mut tokens, &mut buf);
            if !content.is_empty() {
                tokens.push(Tok::Code(content));
            }
            i = end;
            continue;
        }
        if (ch == '!' || ch == '[')
            && let Some((link, end)) = take_link(src, i)
        {
            flush_text(&mut tokens, &mut buf);
            tokens.push(link);
            i = end;
            continue;
        }
        if ch == '<'
            && let Some((url, end)) = angle_link(src, i)
        {
            flush_text(&mut tokens, &mut buf);
            tokens.push(Tok::Link(vec![link_piece(url)], String::new()));
            i = end;
            continue;
        }
        if let Some(end) = bare_url(src, i) {
            flush_text(&mut tokens, &mut buf);
            let url = src[i..end].to_string();
            tokens.push(Tok::Link(vec![link_piece(url)], String::new()));
            i = end;
            continue;
        }
        if matches!(ch, '*' | '_' | '~') {
            let count = run_len(src, i, ch);
            if ch == '~' && count < 2 {
                buf.push(ch);
                i += 1;
                continue;
            }
            let (can_open, can_close) = flanking(src, i, count, ch);
            if !can_open && !can_close {
                buf.push_str(&ch.to_string().repeat(count));
                i += count;
                continue;
            }
            flush_text(&mut tokens, &mut buf);
            tokens.push(Tok::Delim {
                marker: ch,
                count,
                can_open,
                can_close,
            });
            i += count;
            continue;
        }
        if ch.is_control() {
            i += ch.len_utf8();
            continue;
        }
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush_text(&mut tokens, &mut buf);
    tokens
}

enum Tok {
    Text(String),
    Code(String),
    Link(Vec<Piece>, String),
    Image(Vec<Piece>),
    Break,
    Delim {
        marker: char,
        count: usize,
        can_open: bool,
        can_close: bool,
    },
}

fn flush_text(tokens: &mut Vec<Tok>, buf: &mut String) {
    if !buf.is_empty() {
        tokens.push(Tok::Text(std::mem::take(buf)));
    }
}

fn link_piece(text: String) -> Piece {
    Piece {
        text,
        mark: Mark::Link,
    }
}

fn take_link(src: &str, start: usize) -> Option<(Tok, usize)> {
    let image = src[start..].starts_with("![");
    let mut text_at = start;
    if image {
        text_at += 2;
    } else if src[start..].starts_with('[') {
        text_at += 1;
    } else {
        return None;
    }
    let close = find_bracket(src, text_at)?;
    let text = &src[text_at..close];
    let paren = close + 1;
    if !src.get(paren..)?.starts_with('(') {
        return None;
    }
    let (url, rel) = parse_dest(&src[paren + 1..])?;
    let end = paren + 1 + rel;
    if image {
        Some((Tok::Image(image_pieces(text)), end))
    } else {
        Some((Tok::Link(inline(text), url), end))
    }
}

fn find_bracket(src: &str, start: usize) -> Option<usize> {
    let mut depth = 1i32;
    let mut i = start;
    while i < src.len() {
        let ch = next_char(src, i)?;
        if ch == '\\' {
            i += 1;
            if i < src.len() {
                i += next_char(src, i)?.len_utf8();
            }
            continue;
        }
        if ch == '[' {
            depth += 1;
        }
        if ch == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += ch.len_utf8();
    }
    None
}

fn parse_dest(src: &str) -> Option<(String, usize)> {
    if src.is_empty() {
        return None;
    }
    let mut url = String::new();
    let mut i = 0;
    if let Some(rest) = src.strip_prefix('<') {
        let end = rest.find('>')?;
        url = rest[..end].to_string();
        i = end + 2;
    } else {
        let mut depth = 0i32;
        while i < src.len() {
            let ch = next_char(src, i)?;
            if ch == '\\' {
                let next = i + 1;
                let Some(escaped) = next_char(src, next) else {
                    break;
                };
                url.push(escaped);
                i = next + escaped.len_utf8();
                continue;
            }
            if ch.is_whitespace() && depth == 0 {
                break;
            }
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            url.push(ch);
            i += ch.len_utf8();
        }
    }
    while i < src.len() && next_char(src, i).is_some_and(char::is_whitespace) {
        i += next_char(src, i)?.len_utf8();
    }
    if next_char(src, i).is_some_and(|ch| ch == '"' || ch == '\'') {
        let quote = next_char(src, i)?;
        i += quote.len_utf8();
        while i < src.len() {
            let ch = next_char(src, i)?;
            i += ch.len_utf8();
            if ch == '\\' {
                if let Some(escaped) = next_char(src, i) {
                    i += escaped.len_utf8();
                }
                continue;
            }
            if ch == quote {
                break;
            }
        }
        while i < src.len() && next_char(src, i).is_some_and(char::is_whitespace) {
            i += next_char(src, i)?.len_utf8();
        }
    }
    if !src.get(i..)?.starts_with(')') {
        return None;
    }
    Some((url, i + 1))
}

fn image_pieces(alt: &str) -> Vec<Piece> {
    if alt.trim().is_empty() {
        return vec![Piece {
            text: "image".to_string(),
            mark: Mark::Dim,
        }];
    }
    let mut pieces = inline(alt);
    for piece in &mut pieces {
        if piece.mark == Mark::Body {
            piece.mark = Mark::Emphasis;
        }
    }
    pieces
}

fn angle_link(src: &str, start: usize) -> Option<(String, usize)> {
    if !src[start..].starts_with('<') {
        return None;
    }
    let rest = &src[start + 1..];
    let end = rest.find('>')?;
    let inner = &rest[..end];
    if inner.is_empty() || inner.chars().any(char::is_whitespace) {
        return None;
    }
    if !(inner.starts_with("https://")
        || inner.starts_with("http://")
        || inner.starts_with("mailto:"))
    {
        return None;
    }
    Some((inner.to_string(), start + 1 + end + 1))
}

fn bare_url(src: &str, start: usize) -> Option<usize> {
    let rest = &src[start..];
    let scheme = if rest.starts_with("https://") {
        8
    } else if rest.starts_with("http://") {
        7
    } else {
        return None;
    };
    if start > 0
        && src[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    if start + scheme >= src.len() {
        return None;
    }
    let mut end = start + scheme;
    while end < src.len() {
        let ch = next_char(src, end)?;
        if ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\'') {
            break;
        }
        end += ch.len_utf8();
    }
    while end > start + scheme {
        let prev = src[..end].chars().next_back()?;
        let keep_paren = prev == ')' && src[start..end].contains('(');
        if !keep_paren && matches!(prev, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']') {
            end -= prev.len_utf8();
            continue;
        }
        break;
    }
    (end > start + scheme).then_some(end)
}

fn code_span(src: &str, start: usize) -> Option<(String, usize)> {
    if !src[start..].starts_with('`') {
        return None;
    }
    let count = run_len(src, start, '`');
    let mut i = start + count;
    while i < src.len() {
        if src[i..].starts_with('`') {
            let run = run_len(src, i, '`');
            if run == count {
                let mut content = src[start + count..i].replace('\n', " ");
                if content.len() >= 2
                    && content.starts_with(' ')
                    && content.ends_with(' ')
                    && !content.trim().is_empty()
                {
                    content = content[1..content.len() - 1].to_string();
                }
                return Some((content, i + count));
            }
            i += run;
            continue;
        }
        i += next_char(src, i)?.len_utf8();
    }
    None
}

fn run_len(src: &str, start: usize, marker: char) -> usize {
    src[start..].chars().take_while(|ch| *ch == marker).count()
}

fn flanking(src: &str, start: usize, count: usize, marker: char) -> (bool, bool) {
    let after_at = start + count;
    let before = if start == 0 {
        None
    } else {
        src[..start].chars().next_back()
    };
    let after = src.get(after_at..).and_then(|rest| rest.chars().next());
    let before_ws = before.is_none_or(char::is_whitespace);
    let after_ws = after.is_none_or(char::is_whitespace);
    let before_punct = before.is_some_and(is_punct);
    let after_punct = after.is_some_and(is_punct);
    let mut left = !after_ws && (!after_punct || before_ws || before_punct);
    let mut right = !before_ws && (!before_punct || after_ws || after_punct);
    if marker == '_' {
        if before.is_some_and(char::is_alphanumeric) {
            left = false;
        }
        if after.is_some_and(char::is_alphanumeric) {
            right = false;
        }
    }
    (left, right)
}

fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation()
}

struct MatchInfo {
    closer: usize,
    strong: bool,
    em: bool,
    strike: bool,
    left_extra: usize,
    right_extra: usize,
}

fn match_delims(tokens: &[Tok]) -> Vec<Option<MatchInfo>> {
    let mut closer_of = Vec::new();
    for _ in tokens {
        closer_of.push(None);
    }
    let mut dead = vec![false; tokens.len()];
    let mut stack = Vec::new();
    for index in 0..tokens.len() {
        let Tok::Delim {
            marker,
            count,
            can_open,
            can_close,
        } = tokens[index]
        else {
            continue;
        };
        if dead[index] {
            continue;
        }
        if can_close {
            let found = stack.iter().rposition(|&open| {
                if dead[open] {
                    return false;
                }
                matches!(
                    tokens[open],
                    Tok::Delim {
                        marker: open_marker,
                        can_open: true,
                        ..
                    } if open_marker == marker
                )
            });
            if let Some(pos) = found {
                let open = stack[pos];
                let open_count = match tokens[open] {
                    Tok::Delim { count, .. } => count,
                    _ => 0,
                };
                let use_n = open_count.min(count);
                let (strong, em, strike) = delim_flags(marker, use_n);
                if strong || em || strike {
                    closer_of[open] = Some(MatchInfo {
                        closer: index,
                        strong,
                        em,
                        strike,
                        left_extra: open_count - use_n,
                        right_extra: count - use_n,
                    });
                    dead[open + 1..index].fill(true);
                    dead[open] = true;
                    dead[index] = true;
                    stack.truncate(pos);
                    continue;
                }
            }
        }
        if can_open {
            stack.push(index);
        }
    }
    closer_of
}

fn delim_flags(marker: char, count: usize) -> (bool, bool, bool) {
    if marker == '~' {
        (false, false, count >= 2)
    } else {
        (count >= 2, count % 2 == 1, false)
    }
}

fn emit_tokens(tokens: &[Tok], closer_of: &[Option<MatchInfo>]) -> Vec<Piece> {
    emit_range(tokens, closer_of, 0, tokens.len())
}

fn emit_range(
    tokens: &[Tok],
    closer_of: &[Option<MatchInfo>],
    start: usize,
    end: usize,
) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut index = start;
    while index < end {
        if let Some(info) = closer_of.get(index).and_then(Option::as_ref) {
            if info.left_extra > 0 {
                push_text(
                    &mut out,
                    &delim_char(tokens, index)
                        .to_string()
                        .repeat(info.left_extra),
                    Mark::Body,
                );
            }
            let inner = emit_range(tokens, closer_of, index + 1, info.closer);
            out.extend(paint(inner, info.strong, info.em, info.strike));
            if info.right_extra > 0 {
                push_text(
                    &mut out,
                    &delim_char(tokens, info.closer)
                        .to_string()
                        .repeat(info.right_extra),
                    Mark::Body,
                );
            }
            index = info.closer + 1;
            continue;
        }
        match &tokens[index] {
            Tok::Text(text) => push_text(&mut out, text, Mark::Body),
            Tok::Code(text) => push_text(&mut out, text, Mark::Code),
            Tok::Link(pieces, url) => push_link(&mut out, pieces, url),
            Tok::Image(pieces) => out.extend(pieces.clone()),
            Tok::Break => out.push(Piece {
                text: "\n".to_string(),
                mark: Mark::Body,
            }),
            Tok::Delim { marker, count, .. } => {
                push_text(&mut out, &marker.to_string().repeat(*count), Mark::Body);
            }
        }
        index += 1;
    }
    out
}

fn delim_char(tokens: &[Tok], index: usize) -> char {
    match tokens.get(index) {
        Some(Tok::Delim { marker, .. }) => *marker,
        _ => '*',
    }
}

fn push_link(out: &mut Vec<Piece>, pieces: &[Piece], url: &str) {
    let visible: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
    if pieces.is_empty() {
        push_text(out, url, Mark::Link);
    } else {
        for piece in pieces {
            let mark = if piece.mark == Mark::Body {
                Mark::Link
            } else {
                piece.mark
            };
            push_text(out, &piece.text, mark);
        }
    }
    if !url.is_empty() && visible != url {
        push_text(out, &format!(" ({url})"), Mark::Dim);
    }
}

fn push_text(out: &mut Vec<Piece>, text: &str, mark: Mark) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.mark == mark
        && last.text != "\n"
        && text != "\n"
    {
        last.text.push_str(text);
        return;
    }
    out.push(Piece {
        text: text.to_string(),
        mark,
    });
}

fn paint(pieces: Vec<Piece>, strong: bool, em: bool, strike: bool) -> Vec<Piece> {
    pieces
        .into_iter()
        .map(|mut piece| {
            if strike
                && matches!(
                    piece.mark,
                    Mark::Body | Mark::Emphasis | Mark::Strong | Mark::StrongEmphasis
                )
            {
                piece.mark = Mark::Strike;
                return piece;
            }
            piece.mark = compose(piece.mark, strong, em);
            piece
        })
        .collect()
}

fn compose(mark: Mark, strong: bool, em: bool) -> Mark {
    if !strong && !em {
        return mark;
    }
    if matches!(
        mark,
        Mark::Code
            | Mark::CodeBlock
            | Mark::Link
            | Mark::Rule
            | Mark::Strike
            | Mark::Dim
            | Mark::H1
            | Mark::H2
            | Mark::H3
    ) {
        return mark;
    }
    let (was_strong, was_em) = match mark {
        Mark::Strong => (true, false),
        Mark::Emphasis => (false, true),
        Mark::StrongEmphasis => (true, true),
        _ => (false, false),
    };
    match (was_strong || strong, was_em || em) {
        (true, true) => Mark::StrongEmphasis,
        (true, false) => Mark::Strong,
        (false, true) => Mark::Emphasis,
        (false, false) => mark,
    }
}

fn merge_pieces(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut out = Vec::new();
    for piece in pieces {
        push_text(&mut out, &piece.text, piece.mark);
    }
    out
}

fn wrap_pieces(pieces: &[Piece], width: usize, first_prefix: &str, rest_prefix: &str) -> Vec<Row> {
    if width == 0 {
        return Vec::new();
    }
    if pieces.iter().all(|piece| piece.text.is_empty()) {
        if first_prefix.is_empty() {
            return Vec::new();
        }
        return vec![Row {
            pieces: vec![Piece {
                text: first_prefix.to_string(),
                mark: Mark::Body,
            }],
            band: false,
        }];
    }
    let mut wrap = Wrapper {
        width,
        first_prefix,
        rest_prefix,
        lines: Vec::new(),
        line: Vec::new(),
        col: 0,
        word: Vec::new(),
    };
    for piece in pieces {
        if piece.text == "\n" {
            wrap.place_word();
            wrap.flush_line();
            continue;
        }
        if piece.mark == Mark::Code {
            wrap.place_word();
            wrap.word
                .extend(piece.text.chars().map(|ch| (ch, Mark::Code)));
            wrap.place_word();
            continue;
        }
        for ch in piece.text.chars() {
            if ch == '\n' {
                wrap.place_word();
                wrap.flush_line();
                continue;
            }
            if ch.is_whitespace() {
                wrap.place_word();
                continue;
            }
            wrap.word.push((ch, piece.mark));
        }
    }
    wrap.place_word();
    if !wrap.line.is_empty() {
        wrap.flush_line();
    }
    wrap.lines
        .into_iter()
        .enumerate()
        .map(|(index, mut content)| {
            let prefix = if index == 0 {
                first_prefix
            } else {
                rest_prefix
            };
            let mut pieces = Vec::new();
            if !prefix.is_empty() {
                pieces.push(Piece {
                    text: prefix.to_string(),
                    mark: Mark::Body,
                });
            }
            pieces.append(&mut content);
            Row {
                pieces,
                band: false,
            }
        })
        .collect()
}

struct Wrapper<'a> {
    width: usize,
    first_prefix: &'a str,
    rest_prefix: &'a str,
    lines: Vec<Vec<Piece>>,
    line: Vec<(char, Mark)>,
    col: usize,
    word: Vec<(char, Mark)>,
}

impl Wrapper<'_> {
    fn limit(&self) -> usize {
        let prefix = if self.lines.is_empty() {
            self.first_prefix
        } else {
            self.rest_prefix
        };
        self.width.saturating_sub(display_width(prefix)).max(1)
    }

    fn flush_line(&mut self) {
        self.lines.push(chars_to_pieces(&self.line));
        self.line.clear();
        self.col = 0;
    }

    fn place_word(&mut self) {
        while !self.word.is_empty() {
            let limit = self.limit();
            let width_now: usize = self.word.iter().map(|(ch, _)| char_width(*ch)).sum();
            let space = usize::from(!self.line.is_empty());
            if self.line.is_empty() && width_now > limit {
                let chunk = take_chars(&mut self.word, limit);
                self.line.extend(chunk);
                self.flush_line();
                continue;
            }
            if !self.line.is_empty() && self.col + space + width_now > limit {
                self.flush_line();
                continue;
            }
            if !self.line.is_empty() {
                let mark = self
                    .word
                    .first()
                    .map(|(_, mark)| *mark)
                    .unwrap_or(Mark::Body);
                self.line.push((' ', mark));
                self.col += 1;
            }
            self.col += width_now;
            self.line.append(&mut self.word);
        }
    }
}

fn chars_to_pieces(chars: &[(char, Mark)]) -> Vec<Piece> {
    let mut out = Vec::new();
    for (ch, mark) in chars {
        push_text(&mut out, &ch.to_string(), *mark);
    }
    out
}

fn take_chars(word: &mut Vec<(char, Mark)>, limit: usize) -> Vec<(char, Mark)> {
    let mut taken = Vec::new();
    let mut used = 0usize;
    while let Some((ch, mark)) = word.first().copied() {
        let width = char_width(ch);
        if !taken.is_empty() && used + width > limit {
            break;
        }
        word.remove(0);
        taken.push((ch, mark));
        used += width;
        if width > limit {
            break;
        }
    }
    taken
}

fn pieces_width(pieces: &[Piece]) -> usize {
    pieces.iter().map(|piece| text_width(&piece.text)).sum()
}

fn text_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn split_width(text: &str, width: usize) -> (&str, &str) {
    if width == 0 {
        return ("", text);
    }
    let mut used = 0usize;
    for (index, ch) in text.char_indices() {
        let width_ch = char_width(ch);
        if used + width_ch > width && index > 0 {
            return (&text[..index], &text[index..]);
        }
        used += width_ch;
        if used >= width {
            let next = index + ch.len_utf8();
            return (&text[..next], &text[next..]);
        }
    }
    (text, "")
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn strip_spaces(line: &str, count: usize) -> &str {
    let count = leading_spaces(line).min(count);
    &line[count..]
}

fn expand_tabs(text: &str) -> String {
    text.replace('\t', "    ")
}

fn next_char(src: &str, index: usize) -> Option<char> {
    src.get(index..)?.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(rows: &[Row]) -> String {
        rows.iter()
            .map(|row| {
                row.pieces
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn mark_of(rows: &[Row], needle: &str) -> Option<Mark> {
        rows.iter()
            .flat_map(|row| &row.pieces)
            .find(|piece| piece.text.contains(needle))
            .map(|piece| piece.mark)
    }

    #[test]
    fn empty_readme_is_a_note() {
        let rows = render("  \n", 40);
        assert_eq!(flat(&rows), "This README is empty.");
        assert_eq!(mark_of(&rows, "empty"), Some(Mark::Dim));
    }

    #[test]
    fn heading_drops_the_marks_and_keeps_the_paragraph() {
        let rows = render("# Hello\nfrom the repo", 40);
        let text = flat(&rows);
        assert!(text.contains("Hello"), "{text}");
        assert!(text.contains("from the repo"), "{text}");
        assert!(!text.contains('#'), "{text}");
        assert_eq!(mark_of(&rows, "Hello"), Some(Mark::H1));
        assert_eq!(mark_of(&rows, "from the repo"), Some(Mark::Body));
    }

    #[test]
    fn emphasis_code_and_words_keep_their_marks() {
        let rows = render("**alpha** *beta* ***both*** ~~gone~~ `a*b` file_name", 80);
        assert_eq!(mark_of(&rows, "alpha"), Some(Mark::Strong));
        assert_eq!(mark_of(&rows, "beta"), Some(Mark::Emphasis));
        assert_eq!(mark_of(&rows, "both"), Some(Mark::StrongEmphasis));
        assert_eq!(mark_of(&rows, "gone"), Some(Mark::Strike));
        assert_eq!(mark_of(&rows, "a*b"), Some(Mark::Code));
        assert_eq!(mark_of(&rows, "file_name"), Some(Mark::Body));
        let text = flat(&rows);
        assert!(!text.contains("**"), "{text}");
        assert!(!text.contains('`'), "{text}");
        assert!(!text.contains("~~"), "{text}");
    }

    #[test]
    fn fenced_code_stays_literal() {
        let rows = render("```\n**nope**\n```\n", 40);
        assert_eq!(mark_of(&rows, "**nope**"), Some(Mark::CodeBlock));
        assert!(rows.iter().any(|row| row.band));
    }

    #[test]
    fn lists_quotes_rules_and_links_are_formatted() {
        let rows = render(
            "- one\n- [x] done\n\n1. first\n\n> quoted\n\n---\n\nSee [docs](https://example.com).\n",
            60,
        );
        let text = flat(&rows);
        assert!(text.contains("• one"), "{text}");
        assert!(text.contains("☑ done"), "{text}");
        assert!(text.contains("1. first"), "{text}");
        assert!(text.contains("│ "), "{text}");
        assert!(text.contains("quoted"), "{text}");
        assert!(text.contains('─'), "{text}");
        assert!(!text.contains("---"), "{text}");
        assert!(!text.contains("[docs]"), "{text}");
        assert_eq!(mark_of(&rows, "docs"), Some(Mark::Link));
        assert!(text.contains("https://example.com"), "{text}");
    }

    #[test]
    fn nested_list_and_table_and_wrap() {
        let nested = render("- outer\n  - inner\n", 40);
        let text = flat(&nested);
        assert!(text.contains("• outer"), "{text}");
        assert!(text.contains("• inner"), "{text}");
        let table = render("| name | kind |\n| --- | --- |\n| widget | lib |\n", 40);
        let shown = flat(&table);
        assert!(shown.contains("name"), "{shown}");
        assert!(shown.contains("widget"), "{shown}");
        assert!(shown.contains('│'), "{shown}");
        assert_eq!(mark_of(&table, "name"), Some(Mark::Strong));
        let wrapped = render("hello world", 10);
        assert_eq!(
            wrapped
                .iter()
                .map(|row| row
                    .pieces
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>())
                .collect::<Vec<_>>(),
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    #[test]
    fn setext_heading_is_not_a_rule() {
        let rows = render("Hello\n=====\n", 40);
        assert_eq!(mark_of(&rows, "Hello"), Some(Mark::H1));
        assert!(!flat(&rows).contains('='));
    }
}
