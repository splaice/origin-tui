use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                for c in chars.by_ref() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn fit(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(s) <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut col = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + w > width - 1 {
            break;
        }
        out.push(ch);
        col += w;
    }
    out.push('…');
    out
}

/// Split `s` into display-width chunks that concatenate back to `s`.
pub fn wrap_parts(s: &str, width: usize) -> Vec<String> {
    if width == 0 || display_width(s) <= width {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = s;
    while display_width(rest) > width {
        let mut end = 0;
        let mut col = 0;
        let mut soft = None;
        for (i, ch) in rest.char_indices() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w == 0 {
                continue;
            }
            if col + w > width {
                break;
            }
            end = i + ch.len_utf8();
            col += w;
            if matches!(ch, '/' | '-' | '_' | '.' | '?' | '&' | '=' | '#') {
                soft = Some(end);
            }
        }
        let mut cut = end;
        if let Some(soft_at) = soft {
            let soft_width = display_width(&rest[..soft_at]);
            if soft_width >= width / 2 {
                cut = soft_at;
            }
        }
        if cut == 0 {
            if let Some(ch) = rest.chars().next() {
                cut = ch.len_utf8();
            } else {
                break;
            }
        }
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

pub fn format_time(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if raw.len() >= 16 && bytes.get(10) == Some(&b'T') {
        format!("{} {} UTC", &raw[..10], &raw[11..16])
    } else {
        raw.to_string()
    }
}

pub fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '@' | ':' | '+' | '~' | '?')
}

pub fn slice_cols(s: &str, start: usize, end: usize) -> String {
    let mut col = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col >= end {
            break;
        }
        if col >= start {
            out.push(ch);
        }
        col += w;
    }
    out
}

pub fn strip_cols(s: &str, cols: usize) -> String {
    slice_cols(s, cols, usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_round_trips() {
        let url = "https://origin.cursor.com/splaice/plaice-motorsports-technology-planning.git";
        let parts = wrap_parts(url, 22);
        assert!(parts.len() > 1, "{parts:?}");
        assert_eq!(parts.concat(), url);
        for part in &parts {
            assert!(display_width(part) <= 22, "{part}");
        }
    }

    #[test]
    fn fit_marks_truncation() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("ab", 4), "ab");
    }

    #[test]
    fn formats_rfc3339() {
        assert_eq!(
            format_time("2026-08-26T20:30:40.835Z"),
            "2026-08-26 20:30 UTC"
        );
    }

    #[test]
    fn strips_color() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
    }
}
