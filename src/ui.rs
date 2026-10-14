use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, Overlay, ToastKind};
use crate::model::{
    Commit, DetailCursor, DetailLine, DetailSpan, EditKind, Field, FieldId, PullRequest,
    RepoVisibility, VISIBILITIES, detail_lines, on_off, spans_from,
};
use crate::origin::{create_preview, update_preview};
use crate::screen::{FieldHit, Hits, Screen};
use crate::selection;
use crate::text::{fit, format_time, wrap_parts};
use crate::theme::Theme;

const HEADER_H: u16 = 4;
const LABEL_W: u16 = 16;

pub fn draw(frame: &mut Frame, app: &App) -> Screen {
    let theme = Theme::grok_night();
    let Some((mut screen, filter_area)) = paint_main(frame, app, theme) else {
        return Screen::default();
    };
    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => draw_help(frame, theme, &mut screen.hits),
            Overlay::Create(form) => draw_create(frame, form, theme, &mut screen.hits),
            Overlay::Edit(form) => draw_edit(frame, form, theme, &mut screen.hits),
        }
    }
    place_cursor(frame, app, filter_area);
    screen
}

fn paint_main(frame: &mut Frame, app: &App, theme: Theme) -> Option<(Screen, Rect)> {
    let area = frame.area();
    let buf = frame.buffer_mut();
    fill(buf, area, theme.bg, theme.text);
    if let Some(message) = &app.fatal {
        draw_fatal(buf, area, theme, message);
        return None;
    }
    if area.width < 68 || area.height < 16 {
        put(
            buf,
            area.x + 1,
            area.y + 1,
            area.right(),
            "Terminal too small",
            theme.text(),
        );
        put(
            buf,
            area.x + 1,
            area.y + 3,
            area.right(),
            "q quit",
            theme.dim(),
        );
        return None;
    }

    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let left_w = ((main.width as u32 * 34) / 100).clamp(28, 42) as u16;
    let left_w = left_w.min(main.width.saturating_sub(24));
    let [left, divider, right] = Layout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(1),
        Constraint::Min(10),
    ])
    .areas(main);
    for y in divider.y..divider.bottom() {
        buf[(divider.x, y)]
            .set_symbol("│")
            .set_style(theme.fg(theme.border));
    }
    let [left_head, left_body] =
        Layout::vertical([Constraint::Length(HEADER_H), Constraint::Min(1)]).areas(left);
    let [right_head, right_body] =
        Layout::vertical([Constraint::Length(HEADER_H), Constraint::Min(1)]).areas(right);
    let rule_y = left_head.bottom().saturating_sub(1);
    hline(buf, left_head.x, rule_y, left_head.width, theme);
    hline(buf, right_head.x, rule_y, right_head.width, theme);
    buf[(divider.x, rule_y)]
        .set_symbol("┼")
        .set_style(theme.fg(theme.border));

    let mut hits = Hits::default();
    draw_account(buf, left_head, app, theme, &mut hits);
    draw_repo_header(buf, right_head, app, theme, &mut hits);

    let filter_area = Rect {
        x: left_body.x,
        y: left_body.y,
        width: left_body.width,
        height: 1,
    };
    let list_area = Rect {
        x: left_body.x,
        y: left_body.y.saturating_add(1),
        width: left_body.width,
        height: left_body.height.saturating_sub(1),
    };
    hits.list_area = Some(left_body);
    hits.filter = Some(filter_area);
    let list_offset = draw_list(buf, filter_area, list_area, app, theme, &mut hits);

    let (value_width, detail_height, detail_lines, detail_scroll, continues, spans) =
        draw_detail(buf, right_body, app, theme, &mut hits);

    draw_status(buf, status, app, theme);
    if let Some(selection) = app.selection {
        selection::paint(buf, selection, theme.text, theme.selection);
    }

    Some((
        Screen {
            hits,
            continues,
            value_width,
            detail_height,
            detail_lines,
            detail_scroll,
            list_height: list_area.height as usize,
            list_offset,
            spans,
        },
        filter_area,
    ))
}

fn draw_fatal(buf: &mut ratatui::buffer::Buffer, area: Rect, theme: Theme, message: &str) {
    put(
        buf,
        area.x + 2,
        area.y + 2,
        area.right(),
        "ot",
        theme.bold_accent(),
    );
    let mut y = area.y + 4;
    for part in wrap_parts(message, area.width.saturating_sub(4) as usize) {
        if y >= area.bottom() {
            break;
        }
        put(
            buf,
            area.x + 2,
            y,
            area.right().saturating_sub(1),
            &part,
            theme.text(),
        );
        y += 1;
    }
    put(
        buf,
        area.x + 2,
        y.saturating_add(1),
        area.right(),
        "q  quit",
        theme.dim(),
    );
}

fn draw_account(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) {
    let max_x = area.right();
    put(buf, area.x + 1, area.y, max_x, "account", theme.faint());
    let count = format!("{}", app.repos.len());
    let count_x = area
        .right()
        .saturating_sub(1 + UnicodeWidthStr::width(count.as_str()) as u16);
    if count_x > area.x {
        put(buf, count_x, area.y, max_x, &count, theme.faint());
    }
    let email = app
        .auth
        .as_ref()
        .and_then(|auth| auth.account.clone())
        .unwrap_or_else(|| {
            if app.auth_inflight {
                "Loading account…".to_string()
            } else {
                "Not signed in".to_string()
            }
        });
    put(
        buf,
        area.x + 1,
        area.y + 1,
        max_x.saturating_sub(1),
        &email,
        theme.bold(),
    );

    let y = area.y + 2;
    let new_rect = put(buf, area.x + 1, y, max_x, "n", theme.bold_accent());
    let label = put(
        buf,
        new_rect.right() + 1,
        y,
        max_x,
        "new repo",
        theme.text(),
    );
    hits.new_repo = Some(Rect {
        x: new_rect.x,
        y,
        width: label.right().saturating_sub(new_rect.x),
        height: 1,
    });
    let refresh_x = label.right().saturating_add(3);
    let key = put(buf, refresh_x, y, max_x, "r", theme.bold_accent());
    let refresh_label = put(buf, key.right() + 1, y, max_x, "refresh", theme.text());
    hits.refresh = Some(Rect {
        x: key.x,
        y,
        width: refresh_label.right().saturating_sub(key.x).max(1),
        height: 1,
    });
    if let Some(token) = app.auth.as_ref().and_then(|auth| auth.token.clone()) {
        let style = if app.auth.as_ref().is_some_and(|auth| auth.signed_in()) {
            theme.fg(theme.ok)
        } else {
            theme.fg(theme.warn)
        };
        let token_w = UnicodeWidthStr::width(token.as_str()) as u16;
        let token_x = area.right().saturating_sub(1 + token_w);
        if token_x > refresh_label.right() + 1 {
            put(buf, token_x, y, max_x, &token, style);
        }
    } else if let Some(err) = &app.auth_error {
        put(
            buf,
            refresh_label.right() + 2,
            y,
            max_x.saturating_sub(1),
            err,
            theme.fg(theme.danger),
        );
    }
}

fn draw_repo_header(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) {
    let max_x = area.right().saturating_sub(1);
    let label_style = if app.focus == Focus::Detail {
        theme.accent()
    } else {
        theme.faint()
    };
    put(buf, area.x + 1, area.y, max_x, "repository", label_style);
    let title = app
        .selected_repo()
        .map(|repo| repo.full_name.clone())
        .unwrap_or_else(|| "Select a repository".to_string());
    let title_style = if app.selected_repo().is_some() {
        theme.bold()
    } else {
        theme.dim()
    };
    put(buf, area.x + 1, area.y + 1, max_x, &title, title_style);

    let y = area.y + 2;
    let clone_ready = app
        .selected_repo()
        .and_then(|repo| repo.clone_url())
        .is_some();
    let web_ready = app
        .selected_repo()
        .and_then(|repo| repo.web_url())
        .is_some();
    let (clone_key, clone_label) = action_styles(theme, clone_ready);
    let key = put(buf, area.x + 1, y, max_x, "y", clone_key);
    let label = put(buf, key.right() + 1, y, max_x, "copy clone", clone_label);
    hits.copy_clone = Some(Rect {
        x: key.x,
        y,
        width: label.right().saturating_sub(key.x).max(1),
        height: 1,
    });
    let web_x = label.right().saturating_add(3);
    let (web_key_style, web_label_style) = action_styles(theme, web_ready);
    let web_key = put(buf, web_x, y, max_x, "w", web_key_style);
    let web_label = put(
        buf,
        web_key.right() + 1,
        y,
        max_x,
        "copy web",
        web_label_style,
    );
    hits.copy_web = Some(Rect {
        x: web_key.x,
        y,
        width: web_label.right().saturating_sub(web_key.x).max(1),
        height: 1,
    });
    if app.detail_inflight
        && app
            .selected_repo()
            .and_then(|repo| repo.view.as_ref())
            .is_none()
    {
        let note = "loading";
        let note_x = area
            .right()
            .saturating_sub(1 + UnicodeWidthStr::width(note) as u16);
        if note_x > web_label.right() + 1 {
            put(buf, note_x, y, max_x, note, theme.faint());
        }
    }
}

fn action_styles(theme: Theme, enabled: bool) -> (Style, Style) {
    if enabled {
        (theme.bold_accent(), theme.text())
    } else {
        (theme.faint(), theme.faint())
    }
}

fn draw_list(
    buf: &mut ratatui::buffer::Buffer,
    filter_area: Rect,
    list_area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) -> usize {
    let max_x = filter_area.right().saturating_sub(1);
    let focused = app.focus == Focus::Filter && app.overlay.is_none();
    let slash_style = if focused {
        theme.bold_accent()
    } else {
        theme.faint()
    };
    put(
        buf,
        filter_area.x + 1,
        filter_area.y,
        max_x,
        "/",
        slash_style,
    );
    if app.filter.value.is_empty() && !focused {
        put(
            buf,
            filter_area.x + 3,
            filter_area.y,
            max_x,
            "filter",
            theme.faint(),
        );
    } else {
        let room = max_x.saturating_sub(filter_area.x + 3) as usize;
        let (visible, _) = app.filter.window(room);
        put(
            buf,
            filter_area.x + 3,
            filter_area.y,
            max_x,
            &visible,
            theme.text(),
        );
    }

    let rows = app.rows();
    let height = list_area.height as usize;
    let offset = if height == 0 {
        0
    } else {
        app.list_offset.min(rows.len().saturating_sub(height))
    };
    if rows.is_empty() {
        let message = if let Some(err) = &app.list_error {
            err.clone()
        } else if app.list_inflight {
            "Loading repositories…".to_string()
        } else if app.filter.value.is_empty() {
            "No repositories".to_string()
        } else {
            "No matches".to_string()
        };
        let style = if app.list_error.is_some() {
            theme.fg(theme.danger)
        } else {
            theme.dim()
        };
        for (y, part) in (list_area.y..list_area.bottom()).zip(
            wrap_parts(&message, list_area.width.saturating_sub(2) as usize)
                .into_iter()
                .take(4),
        ) {
            put(
                buf,
                list_area.x + 1,
                y,
                list_area.right().saturating_sub(1),
                &part,
                style,
            );
        }
        return offset;
    }

    for (drawn, row) in rows.iter().enumerate().skip(offset).take(height) {
        let y = list_area.y + (drawn - offset) as u16;
        let row_rect = Rect {
            x: list_area.x,
            y,
            width: list_area.width,
            height: 1,
        };
        match row.kind {
            crate::model::RowKind::Header => {
                put(
                    buf,
                    list_area.x + 1,
                    y,
                    list_area.right().saturating_sub(1),
                    &row.label,
                    theme.faint(),
                );
            }
            crate::model::RowKind::Repo(index) => {
                let selected = app.selected == Some(index);
                if selected {
                    fill(buf, row_rect, theme.bg_selected, theme.text);
                }
                let repo = &app.repos[index];
                let badge = repo.badge().unwrap_or("");
                let badge_w = UnicodeWidthStr::width(badge) as u16;
                let right_limit = list_area.right().saturating_sub(1);
                let name_end = if badge_w == 0 {
                    right_limit
                } else {
                    right_limit.saturating_sub(badge_w + 1)
                };
                let mark_x = list_area.x + 1;
                let name_x = mark_x + 2;
                if let Some(visibility) = repo.visibility() {
                    let (glyph, style) = visibility_mark(theme, visibility);
                    put(buf, mark_x, y, name_x, glyph, style);
                }
                let name_style = match (selected, repo.mirrored()) {
                    (true, true) => theme.fg(theme.mirror).add_modifier(Modifier::BOLD),
                    (true, false) => theme.bold_accent(),
                    (false, true) => theme.fg(theme.mirror),
                    (false, false) => theme.text(),
                };
                put(buf, name_x, y, name_end, &repo.name, name_style);
                if badge_w > 0 {
                    put(
                        buf,
                        right_limit.saturating_sub(badge_w),
                        y,
                        list_area.right(),
                        badge,
                        theme.fg(theme.mirror),
                    );
                }
                hits.list_rows.push((row_rect, index));
            }
        }
    }
    offset
}

fn draw_detail(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) -> (
    usize,
    usize,
    usize,
    usize,
    Vec<Option<u16>>,
    Vec<DetailSpan>,
) {
    let mut continues = vec![None; buf.area.height as usize];
    hits.detail_area = Some(area);
    let max_x = area.right().saturating_sub(1);
    let label_x = area.x + 1;
    let value_x = label_x + LABEL_W + 1;
    let value_width = max_x.saturating_sub(value_x) as usize;
    if value_width == 0 {
        return (0, area.height as usize, 0, 0, continues, Vec::new());
    }
    let Some(repo) = app.selected_repo() else {
        put(
            buf,
            area.x + 1,
            area.y,
            max_x,
            "Nothing selected",
            theme.dim(),
        );
        return (
            value_width,
            area.height as usize,
            0,
            0,
            continues,
            Vec::new(),
        );
    };
    if let Some(err) = &repo.detail_error
        && repo.fields().len() <= 1
    {
        for (y, part) in (area.y..area.bottom()).zip(
            wrap_parts(err, area.width.saturating_sub(2) as usize)
                .into_iter()
                .take(6),
        ) {
            put(buf, area.x + 1, y, max_x, &part, theme.fg(theme.danger));
        }
        return (
            value_width,
            area.height as usize,
            0,
            0,
            continues,
            Vec::new(),
        );
    }

    let fields = repo.fields();
    let default_branch = repo.default_branch_name();
    let lines = detail_lines(
        repo,
        value_width,
        app.pulls_inflight,
        app.branches_inflight,
        app.commits_inflight,
    );
    let spans = spans_from(&lines);
    let height = area.height as usize;
    let total = lines.len();
    let scroll = app.detail_scroll.min(total.saturating_sub(height));
    let focused = app.focus == Focus::Detail;
    let mut y = area.y;
    for (index, line) in lines.iter().enumerate() {
        if index < scroll {
            continue;
        }
        if y >= area.bottom() {
            break;
        }
        match line {
            DetailLine::DetailError => {
                if let Some(err) = &repo.detail_error {
                    put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &fit(err, area.width.saturating_sub(2) as usize),
                        theme.fg(theme.danger),
                    );
                }
            }
            DetailLine::Field { id, part } => {
                if let Some(field) = fields.iter().find(|field| field.id == *id) {
                    let parts = wrap_parts(&field.display, value_width);
                    let text = parts.get(*part).map(String::as_str).unwrap_or("");
                    let selected = focused
                        && matches!(app.cursor, DetailCursor::Field)
                        && field.id == app.selected_field;
                    if selected {
                        fill(
                            buf,
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            theme.bg_selected,
                            theme.text,
                        );
                    }
                    if *part == 0 {
                        let label_style = if selected && field.id.editable() {
                            theme.accent()
                        } else if selected {
                            theme.text()
                        } else {
                            theme.dim()
                        };
                        put(buf, label_x, y, label_x + LABEL_W, field.label, label_style);
                    } else if (y as usize) < continues.len() {
                        continues[y as usize] = Some(value_x);
                    }
                    let rect = put(
                        buf,
                        value_x,
                        y,
                        max_x,
                        text,
                        field_style(theme, field, selected),
                    );
                    if rect.width > 0 {
                        hits.fields.push(FieldHit { id: field.id, rect });
                    }
                }
            }
            DetailLine::Gap => {}
            DetailLine::BranchesHeading => {
                put(buf, area.x + 1, y, max_x, "branches", theme.faint());
            }
            DetailLine::BranchesLoading => {
                put(buf, area.x + 1, y, max_x, "Loading branches…", theme.dim());
            }
            DetailLine::BranchesError => {
                if let Some(err) = &repo.branches_error {
                    put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &fit(err, area.width.saturating_sub(2) as usize),
                        theme.fg(theme.danger),
                    );
                }
            }
            DetailLine::Branch(index) => {
                if let Some(branch) = repo.branches.get(*index) {
                    let chosen = app.selected_branch.as_deref() == Some(branch.name.as_str());
                    let selected =
                        focused && matches!(app.cursor, DetailCursor::Branch(at) if at == *index);
                    let row = Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    };
                    if selected {
                        fill(buf, row, theme.bg_selected, theme.text);
                    }
                    let mark = if chosen { "› " } else { "  " };
                    let name_style = if selected {
                        theme.text().bg(theme.bg_selected)
                    } else if chosen {
                        theme.accent()
                    } else {
                        theme.text()
                    };
                    let name = put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &format!("{mark}{}", branch.name),
                        name_style,
                    );
                    if default_branch.as_deref() == Some(branch.name.as_str()) {
                        let mut mark_style = theme.faint();
                        if selected {
                            mark_style = mark_style.bg(theme.bg_selected);
                        }
                        put(
                            buf,
                            name.right().saturating_add(2),
                            y,
                            max_x,
                            "default",
                            mark_style,
                        );
                    }
                    hits.branch_rows.push((row, *index));
                }
            }
            DetailLine::CommitsHeading => {
                let title = if repo.branches.len() > 1 {
                    app.selected_branch
                        .as_ref()
                        .map(|name| format!("commits  {name}"))
                        .unwrap_or_else(|| "commits".to_string())
                } else {
                    "commits".to_string()
                };
                put(buf, area.x + 1, y, max_x, &title, theme.faint());
            }
            DetailLine::CommitsLoading => {
                put(buf, area.x + 1, y, max_x, "Loading commits…", theme.dim());
            }
            DetailLine::CommitsError => {
                if let Some(err) = &repo.commits_error {
                    put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &fit(err, area.width.saturating_sub(2) as usize),
                        theme.fg(theme.danger),
                    );
                }
            }
            DetailLine::CommitsEmpty => {
                put(buf, area.x + 1, y, max_x, "No commits", theme.dim());
            }
            DetailLine::Commit(index) => {
                if let Some(commit) = repo.commits.get(*index) {
                    let selected =
                        focused && matches!(app.cursor, DetailCursor::Commit(at) if at == *index);
                    let row = Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    };
                    if selected {
                        fill(buf, row, theme.bg_selected, theme.text);
                    }
                    let sha = short_sha(&commit.sha);
                    let sha_rect = put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &sha,
                        painted(theme, theme.accent(), selected),
                    );
                    put(
                        buf,
                        sha_rect.right().saturating_add(2),
                        y,
                        max_x,
                        &commit_tail(commit),
                        painted(theme, theme.text(), selected),
                    );
                    hits.commit_rows.push((row, *index));
                }
            }
            DetailLine::CommitsCapped => {
                put(
                    buf,
                    area.x + 1,
                    y,
                    max_x,
                    "Showing the latest 20",
                    theme.faint(),
                );
            }
            DetailLine::Heading => {
                put(buf, area.x + 1, y, max_x, "pull requests", theme.faint());
            }
            DetailLine::PullsError => {
                if let Some(err) = &repo.pulls_error {
                    put(
                        buf,
                        area.x + 1,
                        y,
                        max_x,
                        &fit(err, area.width.saturating_sub(2) as usize),
                        theme.fg(theme.danger),
                    );
                }
            }
            DetailLine::Loading => {
                put(
                    buf,
                    area.x + 1,
                    y,
                    max_x,
                    "Loading pull requests…",
                    theme.dim(),
                );
            }
            DetailLine::Empty => {
                put(buf, area.x + 1, y, max_x, "No pull requests", theme.dim());
            }
            DetailLine::Pull(pull_index) => {
                if let Some(pull) = repo.pulls.get(*pull_index) {
                    let selected = focused
                        && matches!(app.cursor, DetailCursor::Pull(at) if at == *pull_index);
                    let row = Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    };
                    if selected {
                        fill(buf, row, theme.bg_selected, theme.text);
                    }
                    let mut style = pull_style(theme, &pull.status);
                    if selected {
                        style = style.bg(theme.bg_selected);
                    }
                    put(buf, area.x + 1, y, max_x, &pull_line(pull), style);
                    hits.pull_rows.push((row, *pull_index));
                }
            }
            DetailLine::Capped => {
                put(
                    buf,
                    area.x + 1,
                    y,
                    max_x,
                    "Showing the latest 50",
                    theme.faint(),
                );
            }
        }
        y += 1;
    }
    (value_width, height, total, scroll, continues, spans)
}

fn painted(theme: Theme, style: Style, selected: bool) -> Style {
    if selected {
        style.bg(theme.bg_selected)
    } else {
        style
    }
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn commit_tail(commit: &Commit) -> String {
    let summary = if commit.summary.is_empty() {
        "(no message)"
    } else {
        commit.summary.as_str()
    };
    let mut text = summary.to_string();
    if !commit.author.is_empty() {
        text.push_str("  ·  ");
        text.push_str(&commit.author);
    }
    if !commit.date.is_empty() {
        text.push_str("  ·  ");
        text.push_str(&format_time(&commit.date));
    }
    text
}

fn visibility_mark(theme: Theme, visibility: RepoVisibility) -> (&'static str, Style) {
    match visibility {
        RepoVisibility::Public => ("+", theme.fg(theme.ok)),
        RepoVisibility::Private => ("·", theme.dim()),
        RepoVisibility::Internal => ("~", theme.fg(theme.warn)),
    }
}

fn pull_line(pull: &PullRequest) -> String {
    format!("#{}  {}  {}", pull.number, pull.status, pull.title)
}

fn pull_style(theme: Theme, status: &str) -> Style {
    match status {
        "open" => theme.fg(theme.ok),
        "draft" => theme.fg(theme.warn),
        "merged" => theme.fg(theme.mirror),
        "closed" => theme.dim(),
        _ => theme.text(),
    }
}

fn field_style(theme: Theme, field: &Field, selected: bool) -> Style {
    match field.id {
        FieldId::Visibility if field.display == "public" => theme.fg(theme.ok),
        FieldId::Visibility if field.display == "internal" => theme.fg(theme.warn),
        FieldId::Visibility => theme.dim(),
        FieldId::Mirror if field.display != "origin" => theme.fg(theme.mirror),
        FieldId::MergeCommit | FieldId::Squash | FieldId::DeleteBranch if field.display == "on" => {
            theme.fg(theme.ok)
        }
        FieldId::MergeCommit | FieldId::Squash | FieldId::DeleteBranch => theme.dim(),
        FieldId::Clone | FieldId::Web if !selected => theme.text(),
        _ if selected => theme.text(),
        _ => theme.text(),
    }
}

fn draw_status(buf: &mut ratatui::buffer::Buffer, area: Rect, app: &App, theme: Theme) {
    fill(buf, area, theme.bg_raised, theme.dim);
    let toast = app.toast.as_ref().map(|toast| {
        let style = match toast.kind {
            ToastKind::Info => theme.fg(theme.accent),
            ToastKind::Err => theme.fg(theme.danger),
        };
        (toast.text.clone(), style)
    });
    let right = toast.unwrap_or_else(|| (count_label(app), theme.faint()));
    let right_w = UnicodeWidthStr::width(right.0.as_str()) as u16;
    let right_x = area.right().saturating_sub(1 + right_w);
    let hint_end = right_x.saturating_sub(2);
    draw_hints(buf, area.x + 1, area.y, hint_end, app, theme);
    if right_x > area.x {
        put(buf, right_x, area.y, area.right(), &right.0, right.1);
    }
}

fn count_label(app: &App) -> String {
    if app.list_inflight && app.repos.is_empty() {
        return "loading".to_string();
    }
    if app.list_inflight {
        return "refreshing".to_string();
    }
    let total = app.repos.len();
    let shown = app.visible_indices().len();
    if app.filter.value.trim().is_empty() {
        format!("{total} {}", if total == 1 { "repo" } else { "repos" })
    } else {
        format!("{shown} of {total}")
    }
}

fn draw_hints(
    buf: &mut ratatui::buffer::Buffer,
    mut x: u16,
    y: u16,
    max_x: u16,
    app: &App,
    theme: Theme,
) {
    let hints: &[(&str, bool)] = match (&app.overlay, app.focus) {
        (Some(Overlay::Help), _) => &[
            ("esc", true),
            (" close", false),
            ("   ", false),
            ("q", true),
            (" quit", false),
        ],
        (Some(Overlay::Create(_)), _) => &[
            ("tab", true),
            (" field", false),
            ("   ", false),
            ("enter", true),
            (" create", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
        (Some(Overlay::Edit(form)), _) => edit_hints(&form.kind),
        (_, Focus::Filter) => &[
            ("type to filter", false),
            ("   ", false),
            ("↑↓", true),
            (" repos", false),
            ("   ", false),
            ("enter", true),
            (" done", false),
            ("   ", false),
            ("esc", true),
            (" clear", false),
        ],
        (_, Focus::Detail) => &[
            ("↑↓", true),
            (" row", false),
            ("   ", false),
            ("e", true),
            (" edit", false),
            ("   ", false),
            ("y", true),
            (" copy", false),
            ("   ", false),
            ("w", true),
            (" web", false),
            ("   ", false),
            ("tab", true),
            (" list", false),
            ("   ", false),
            ("n", true),
            (" new", false),
            ("   ", false),
            ("?", true),
            (" help", false),
        ],
        (_, Focus::List) => &[
            ("↑↓", true),
            (" move", false),
            ("   ", false),
            ("/", true),
            (" filter", false),
            ("   ", false),
            ("tab", true),
            (" details", false),
            ("   ", false),
            ("n", true),
            (" new", false),
            ("   ", false),
            ("y", true),
            (" copy", false),
            ("   ", false),
            ("w", true),
            (" web", false),
            ("   ", false),
            ("?", true),
            (" help", false),
        ],
    };
    for (text, key) in hints {
        if x >= max_x {
            break;
        }
        let style = if *key { theme.accent() } else { theme.dim() };
        let rect = put(buf, x, y, max_x, text, style);
        if rect.width == 0 {
            break;
        }
        x = rect.right();
    }
}

fn draw_help(frame: &mut Frame, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 56, 18);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(" Keys ", theme.bold_accent())))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let buf = frame.buffer_mut();
    fill(buf, inner, theme.bg_raised, theme.text);
    let rows = [
        ("Move", "↑ ↓    j k"),
        ("Page", "pgup pgdn"),
        ("Jump", "g G    home end"),
        ("Panes", "tab    ← →"),
        ("Filter", "/"),
        ("New repository", "n"),
        ("Refresh", "r"),
        ("Copy focused", "y    enter"),
        ("Copy web URL", "w"),
        ("Edit setting", "e"),
        ("Visibility", "+ public  · private  ~ internal"),
        ("Select text", "drag"),
        ("Copy a value", "double-click"),
        ("Close this", "esc"),
        ("Quit", "q    ctrl-c"),
    ];
    for (index, (label, keys)) in rows.iter().enumerate() {
        let y = inner.y + index as u16;
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner.x + 1, y, inner.x + 18, label, theme.dim());
        put(
            buf,
            inner.x + 18,
            y,
            inner.right().saturating_sub(1),
            keys,
            theme.text(),
        );
    }
}

fn draw_create(frame: &mut Frame, form: &crate::model::CreateForm, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 64, 16);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(
            " Create repository ",
            theme.bold_accent(),
        )))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let cursor = {
        let buf = frame.buffer_mut();
        fill(buf, inner, theme.bg_raised, theme.text);
        let fields = [
            ("Name", &form.name, form.focus == 0),
            ("Namespace", &form.namespace, form.focus == 1),
            ("Default branch", &form.branch, form.focus == 2),
        ];
        let mut y = inner.y;
        let mut cursor = None;
        for (label, editor, focused) in fields {
            if y + 1 >= inner.bottom() {
                break;
            }
            put(
                buf,
                inner.x + 1,
                y,
                inner.right().saturating_sub(1),
                label,
                theme.dim(),
            );
            y += 1;
            let row = Rect {
                x: inner.x + 1,
                y,
                width: inner.width.saturating_sub(2),
                height: 1,
            };
            let bg = if focused {
                theme.bg_selected
            } else {
                theme.bg_field
            };
            fill(buf, row, bg, theme.text);
            let room = row.width.saturating_sub(1) as usize;
            let (visible, col) = editor.window(room);
            put(buf, row.x + 1, y, row.right(), &visible, theme.text());
            hits.modal_inputs.push(row);
            if focused && !form.busy {
                let cursor_x = row.x + 1 + col as u16;
                if cursor_x < row.right() {
                    cursor = Some((cursor_x, y));
                }
            }
            y = y.saturating_add(2);
        }
        let preview = if form.busy {
            "Creating…".to_string()
        } else {
            create_preview(
                form.namespace.value.trim(),
                form.name.value.trim(),
                form.branch.value.trim(),
            )
        };
        if y < inner.bottom() {
            put(
                buf,
                inner.x + 1,
                y,
                inner.right().saturating_sub(1),
                &fit(&preview, inner.width.saturating_sub(2) as usize),
                theme.faint(),
            );
            y += 1;
        }
        if let Some(err) = &form.error
            && y < inner.bottom()
        {
            put(
                buf,
                inner.x + 1,
                y,
                inner.right().saturating_sub(1),
                &fit(err, inner.width.saturating_sub(2) as usize),
                theme.fg(theme.danger),
            );
        }
        cursor
    };
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn edit_hints(kind: &EditKind) -> &'static [(&'static str, bool)] {
    match kind {
        EditKind::Visibility { .. } => &[
            ("↑↓", true),
            (" choose", false),
            ("   ", false),
            ("enter", true),
            (" save", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
        EditKind::Branch(_) | EditKind::Confirm { .. } => &[
            ("enter", true),
            (" save", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
    }
}

fn draw_edit(frame: &mut Frame, form: &crate::model::EditForm, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 62, 12);
    hits.modal = Some(area);
    let title = match &form.kind {
        EditKind::Visibility { .. } => " Visibility ",
        EditKind::Branch(_) => " Default branch ",
        EditKind::Confirm { .. } => " Save setting ",
    };
    let block = Block::default()
        .title(Line::from(Span::styled(title, theme.bold_accent())))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let cursor = {
        let buf = frame.buffer_mut();
        fill(buf, inner, theme.bg_raised, theme.text);
        let mut y = inner.y;
        let mut cursor = None;
        let max_x = inner.right().saturating_sub(1);
        match &form.kind {
            EditKind::Visibility { current, choice } => {
                if y < inner.bottom() {
                    put(
                        buf,
                        inner.x + 1,
                        y,
                        max_x,
                        &format!("now {current}"),
                        theme.dim(),
                    );
                    y = y.saturating_add(2);
                }
                for (index, name) in VISIBILITIES.iter().enumerate() {
                    if y >= inner.bottom() {
                        break;
                    }
                    let row = Rect {
                        x: inner.x + 1,
                        y,
                        width: inner.width.saturating_sub(2),
                        height: 1,
                    };
                    let selected = *choice == index;
                    fill(
                        buf,
                        row,
                        if selected {
                            theme.bg_selected
                        } else {
                            theme.bg_field
                        },
                        theme.text,
                    );
                    let style = if selected {
                        theme.accent()
                    } else {
                        theme.text()
                    };
                    put(buf, row.x + 1, y, row.right(), name, style);
                    hits.modal_inputs.push(row);
                    y += 1;
                }
                y += 1;
            }
            EditKind::Branch(editor) => {
                if y < inner.bottom() {
                    put(buf, inner.x + 1, y, max_x, "Branch", theme.dim());
                    y += 1;
                }
                if y < inner.bottom() {
                    let row = Rect {
                        x: inner.x + 1,
                        y,
                        width: inner.width.saturating_sub(2),
                        height: 1,
                    };
                    fill(buf, row, theme.bg_selected, theme.text);
                    let room = row.width.saturating_sub(1) as usize;
                    let (visible, col) = editor.window(room);
                    put(buf, row.x + 1, y, row.right(), &visible, theme.text());
                    hits.modal_inputs.push(row);
                    if !form.busy {
                        let cursor_x = row.x + 1 + col as u16;
                        if cursor_x < row.right() {
                            cursor = Some((cursor_x, y));
                        }
                    }
                    y = y.saturating_add(2);
                }
            }
            EditKind::Confirm {
                label, from, to, ..
            } => {
                if y < inner.bottom() {
                    put(buf, inner.x + 1, y, max_x, label, theme.dim());
                    y += 1;
                }
                if y < inner.bottom() {
                    put(
                        buf,
                        inner.x + 1,
                        y,
                        max_x,
                        &format!("{}  →  {}", on_off(*from), on_off(*to)),
                        theme.text(),
                    );
                    y = y.saturating_add(2);
                }
            }
        }
        let preview = if form.busy {
            "Saving…".to_string()
        } else {
            update_preview(form.full_name.trim(), &form.patch())
        };
        if y < inner.bottom() {
            put(
                buf,
                inner.x + 1,
                y,
                max_x,
                &fit(&preview, inner.width.saturating_sub(2) as usize),
                theme.faint(),
            );
            y += 1;
        }
        if let Some(err) = &form.error
            && y < inner.bottom()
        {
            put(
                buf,
                inner.x + 1,
                y,
                max_x,
                &fit(err, inner.width.saturating_sub(2) as usize),
                theme.fg(theme.danger),
            );
        }
        cursor
    };
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn place_cursor(frame: &mut Frame, app: &App, filter_area: Rect) {
    if app.overlay.is_some() || app.focus != Focus::Filter {
        return;
    }
    let room = filter_area.width.saturating_sub(4) as usize;
    let (_, col) = app.filter.window(room);
    let x = filter_area.x + 3 + col as u16;
    let max_x = filter_area.right().saturating_sub(1);
    if x < max_x {
        frame.set_cursor_position((x, filter_area.y));
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn hline(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, width: u16, theme: Theme) {
    for col in x..x.saturating_add(width) {
        buf[(col, y)]
            .set_symbol("─")
            .set_style(theme.fg(theme.border));
    }
}

fn fill(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    bg: ratatui::style::Color,
    fg: ratatui::style::Color,
) {
    let style = Style::new().bg(bg).fg(fg);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn put(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    max_x: u16,
    text: &str,
    style: Style,
) -> Rect {
    if y >= buf.area.height || x >= max_x || x >= buf.area.width {
        return Rect {
            x,
            y,
            width: 0,
            height: 1,
        };
    }
    let max_x = max_x.min(buf.area.width);
    let fitted = fit(text, (max_x - x) as usize);
    let width = UnicodeWidthStr::width(fitted.as_str()) as u16;
    if width > 0 {
        buf.set_string(x, y, &fitted, style);
    }
    Rect {
        x,
        y,
        width,
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::model::{
        ApiRepo, AuthStatus, Branch, Commit, ListedRepo, PullRequest, Repo, ViewRepo,
    };
    use crate::selection::{self, Selection};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn mirrored(name: &str) -> Repo {
        let mut repo = Repo::from_listed(ListedRepo {
            owner: "splaice".to_string(),
            name: name.to_string(),
            clone_url: format!("https://origin.cursor.com/splaice/{name}.git"),
        });
        repo.apply_summary(ApiRepo {
            id: Some("r_1".to_string()),
            full_name: format!("splaice/{name}"),
            default_branch: Some("main".to_string()),
            owner_slug: Some("splaice".to_string()),
            owner_kind: Some("user".to_string()),
            clone_url: Some(repo.listed_clone.clone()),
            web_url: Some(format!("https://cursor.com/codebase/splaice/{name}")),
            visibility: Some("private".to_string()),
            mirror_source: Some("github".to_string()),
            mirror_status: Some("inbound".to_string()),
            allow_merge_commit: Some(true),
            allow_squash_merge: Some(true),
            delete_branch_on_merge: Some(false),
            ..ApiRepo::default()
        });
        repo
    }

    fn app_with(repos: Vec<Repo>) -> App {
        let mut app = App::default();
        app.auth = Some(AuthStatus {
            account: Some("splaice@gmail.com".to_string()),
            method: Some("login".to_string()),
            token: Some("valid".to_string()),
            endpoint: None,
        });
        app.repos = repos;
        app.selected = Some(0);
        app
    }

    fn render(app: &App, width: u16, height: u16) -> (ratatui::buffer::Buffer, Screen) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut screen = Screen::default();
        terminal
            .draw(|frame| {
                screen = draw(frame, app);
            })
            .unwrap();
        (terminal.backend().buffer().clone(), screen)
    }

    fn plain(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            out.push_str(&selection::row_text(buf, y, 0, buf.area.width - 1));
            out.push('\n');
        }
        out
    }

    #[test]
    fn layout_shows_account_repo_and_actions() {
        let app = app_with(vec![mirrored("dotfiles"), mirrored("garage")]);
        let (buf, _) = render(&app, 110, 32);
        let text = plain(&buf);
        assert!(text.contains("splaice@gmail.com"), "{text}");
        assert!(text.contains("dotfiles"), "{text}");
        assert!(text.contains("copy clone"), "{text}");
        assert!(text.contains("copy web"), "{text}");
        assert!(text.contains("new repo"), "{text}");
        assert!(text.contains("private"), "{text}");
        let left = list_line(&buf, "dotfiles");
        let left = left.split('│').next().unwrap_or(&left);
        assert!(left.trim_end().ends_with('m'), "{left}");
        assert!(left.contains('·'), "{left}");
    }

    #[test]
    fn mirror_names_use_a_different_color() {
        let app = app_with(vec![mirrored("dotfiles"), native("garage")]);
        let (buf, _) = render(&app, 110, 32);
        let theme = Theme::grok_night();
        assert_eq!(name_fg(&buf, "dotfiles"), Some(theme.mirror));
        assert_eq!(name_fg(&buf, "garage"), Some(theme.text));
        let garage = list_line(&buf, "garage");
        assert!(!garage.trim_end().ends_with('m'), "{garage}");
    }

    fn native(name: &str) -> Repo {
        Repo::from_listed(ListedRepo {
            owner: "splaice".to_string(),
            name: name.to_string(),
            clone_url: format!("https://origin.cursor.com/splaice/{name}.git"),
        })
    }

    fn name_pos(buf: &ratatui::buffer::Buffer, name: &str) -> Option<(u16, u16)> {
        for y in 0..buf.area.height {
            let line = selection::row_text(buf, y, 0, buf.area.width - 1);
            if let Some(col) = token_at(&line, name) {
                return Some((col as u16, y));
            }
        }
        None
    }

    fn token_at(line: &str, name: &str) -> Option<usize> {
        let chars: Vec<char> = line.chars().collect();
        let needle: Vec<char> = name.chars().collect();
        if needle.is_empty() || chars.len() < needle.len() {
            return None;
        }
        for index in 0..=chars.len() - needle.len() {
            if chars[index..index + needle.len()] != needle[..] {
                continue;
            }
            let before = index == 0 || chars[index - 1].is_whitespace();
            let after = index + needle.len();
            if before && (after == chars.len() || chars[after].is_whitespace()) {
                return Some(index);
            }
        }
        None
    }

    fn list_line(buf: &ratatui::buffer::Buffer, name: &str) -> String {
        name_pos(buf, name)
            .map(|(_, y)| selection::row_text(buf, y, 0, buf.area.width - 1))
            .unwrap_or_default()
    }

    fn name_fg(buf: &ratatui::buffer::Buffer, name: &str) -> Option<ratatui::style::Color> {
        name_pos(buf, name).and_then(|(x, y)| buf[(x, y)].style().fg)
    }

    fn visibility_glyph(
        buf: &ratatui::buffer::Buffer,
        name: &str,
    ) -> Option<(char, ratatui::style::Color)> {
        let (x, y) = name_pos(buf, name)?;
        if x < 2 {
            return None;
        }
        let cell = &buf[(x - 2, y)];
        let glyph = cell.symbol().chars().next()?;
        if glyph.is_whitespace() {
            return None;
        }
        Some((glyph, cell.style().fg?))
    }

    #[test]
    fn wrapped_clone_url_copies_as_one_string() {
        let url = "https://origin.cursor.com/splaice/plaice-motorsports-technology-planning.git";
        let app = app_with(vec![mirrored("plaice-motorsports-technology-planning")]);
        let (buf, screen) = render(&app, 100, 36);
        let lines: Vec<_> = screen
            .hits
            .fields
            .iter()
            .filter(|hit| hit.id == FieldId::Clone)
            .map(|hit| hit.rect)
            .collect();
        assert!(!lines.is_empty(), "{}", plain(&buf));
        let last = lines.last().unwrap();
        let selection = Selection::new((lines[0].x, lines[0].y), (last.x + last.width - 1, last.y));
        let copied = selection::extract(&buf, selection, &screen.continues);
        assert_eq!(copied, url, "screen:\n{}", plain(&buf));
    }

    #[test]
    fn detail_lists_open_and_closed_pulls() {
        let mut repo = native("widget");
        repo.pulls = vec![
            PullRequest {
                number: 9,
                title: "Add gate".to_string(),
                status: "open".to_string(),
                url: "https://example.com/9".to_string(),
            },
            PullRequest {
                number: 2,
                title: "Old fix".to_string(),
                status: "closed".to_string(),
                url: "https://example.com/2".to_string(),
            },
        ];
        let app = app_with(vec![repo]);
        let (buf, _) = render(&app, 110, 32);
        let text = plain(&buf);
        assert!(text.contains("pull requests"), "{text}");
        assert!(text.contains("#9  open  Add gate"), "{text}");
        assert!(text.contains("#2  closed  Old fix"), "{text}");
    }

    fn history_commit() -> Commit {
        Commit {
            sha: "abcdef0123456789".to_string(),
            summary: "Add gate".to_string(),
            author: "Ada".to_string(),
            date: "2026-10-01T12:00:00Z".to_string(),
            url: String::new(),
        }
    }

    #[test]
    fn detail_lists_branches_and_their_commits() {
        let mut repo = native("widget");
        repo.view = Some(ViewRepo {
            default_branch: Some("main".to_string()),
            ..ViewRepo::default()
        });
        repo.branches = vec![
            Branch {
                name: "main".to_string(),
                sha: "aaa".to_string(),
            },
            Branch {
                name: "topic".to_string(),
                sha: "bbb".to_string(),
            },
        ];
        repo.commits = vec![history_commit()];
        repo.commits_for = Some("main".to_string());
        let mut app = app_with(vec![repo]);
        app.selected_branch = Some("main".to_string());
        app.focus = Focus::Detail;
        let (buf, screen) = render(&app, 110, 40);
        let text = plain(&buf);
        assert!(text.contains("branches"), "{text}");
        assert!(text.contains("main"), "{text}");
        assert!(text.contains("topic"), "{text}");
        assert!(text.contains("default"), "{text}");
        assert!(text.contains("commits  main"), "{text}");
        assert!(text.contains("abcdef0"), "{text}");
        assert!(text.contains("Add gate"), "{text}");
        assert!(text.contains("Ada"), "{text}");
        assert_eq!(screen.hits.branch_rows.len(), 2);
        assert_eq!(screen.hits.commit_rows.len(), 1);
    }

    #[test]
    fn one_branch_shows_commits_without_a_branch_list() {
        let mut repo = native("widget");
        repo.branches = vec![Branch {
            name: "main".to_string(),
            sha: "aaa".to_string(),
        }];
        repo.commits = vec![history_commit()];
        repo.commits_for = Some("main".to_string());
        let app = app_with(vec![repo]);
        let (buf, screen) = render(&app, 110, 40);
        let text = plain(&buf);
        assert!(text.contains("commits"), "{text}");
        assert!(text.contains("abcdef0"), "{text}");
        assert!(text.contains("Add gate"), "{text}");
        assert!(!text.contains("branches"), "{text}");
        assert!(screen.hits.branch_rows.is_empty());
        assert_eq!(screen.hits.commit_rows.len(), 1);
    }

    fn listed(name: &str, visibility: Option<&str>) -> Repo {
        let mut repo = native(name);
        if let Some(visibility) = visibility {
            repo.apply_summary(ApiRepo {
                full_name: format!("splaice/{name}"),
                visibility: Some(visibility.to_string()),
                ..ApiRepo::default()
            });
        }
        repo
    }

    #[test]
    fn list_marks_public_private_and_internal() {
        let app = app_with(vec![
            listed("open", Some("public")),
            listed("closed", Some("private")),
            listed("shared", Some("internal")),
            listed("loading", None),
        ]);
        let (buf, _) = render(&app, 110, 32);
        let theme = Theme::grok_night();
        assert_eq!(visibility_glyph(&buf, "open"), Some(('+', theme.ok)));
        assert_eq!(visibility_glyph(&buf, "closed"), Some(('·', theme.dim)));
        assert_eq!(visibility_glyph(&buf, "shared"), Some(('~', theme.warn)));
        assert_eq!(visibility_glyph(&buf, "loading"), None);
        let open = name_pos(&buf, "open").unwrap().0;
        let closed = name_pos(&buf, "closed").unwrap().0;
        let shared = name_pos(&buf, "shared").unwrap().0;
        let loading = name_pos(&buf, "loading").unwrap().0;
        assert_eq!(open, closed);
        assert_eq!(closed, shared);
        assert_eq!(shared, loading);
    }
}
