use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::buffer::Buffer;

use crate::model::{
    ApiRepo, AuthStatus, CreateForm, DetailCursor, DetailSpan, DetailTarget, DisplayRow, EditForm,
    EditKind, Editor, FieldId, ListedRepo, Namespace, Repo, RepoPatch, RowKind, VISIBILITIES,
    ViewRepo, detail_lines, display_rows, order_branches, preferred_branch, spans_from,
};
use crate::origin;
use crate::screen::{FieldHit, Screen, contains};
use crate::selection::{self, Selection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
    Filter,
}

pub enum Overlay {
    Help,
    Create(CreateForm),
    Edit(EditForm),
}

pub enum Job {
    Auth,
    Namespaces,
    RepoList {
        generation: u64,
    },
    Enrich {
        owner: String,
    },
    Detail {
        full_name: String,
        generation: u64,
    },
    Branches {
        full_name: String,
        generation: u64,
    },
    Commits {
        full_name: String,
        branch: String,
        generation: u64,
    },
    Pulls {
        full_name: String,
        generation: u64,
    },
    Update {
        full_name: String,
        generation: u64,
        patch: RepoPatch,
    },
    Create {
        generation: u64,
        namespace: String,
        name: String,
        branch: String,
    },
}

pub enum Msg {
    Auth(Result<AuthStatus, String>),
    Namespaces(Result<Vec<Namespace>, String>),
    RepoList {
        generation: u64,
        result: Result<Vec<ListedRepo>, String>,
    },
    Enrich {
        owner: String,
        result: Result<Vec<ApiRepo>, String>,
    },
    Detail {
        generation: u64,
        full_name: String,
        view: Box<Result<ViewRepo, String>>,
        api: Box<Result<ApiRepo, String>>,
    },
    Branches {
        generation: u64,
        full_name: String,
        result: Result<Vec<crate::model::Branch>, String>,
    },
    Commits {
        generation: u64,
        full_name: String,
        branch: String,
        result: Result<Vec<crate::model::Commit>, String>,
    },
    Pulls {
        generation: u64,
        full_name: String,
        result: Result<Vec<crate::model::PullRequest>, String>,
    },
    Updated {
        generation: u64,
        full_name: String,
        result: Box<Result<ApiRepo, String>>,
    },
    Created {
        generation: u64,
        full_name: String,
        result: Result<(), String>,
    },
}

pub enum Effect {
    None,
    Quit,
    Copy { text: String, toast: String },
    Jobs(Vec<Job>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Err,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

struct Press {
    x: u16,
    y: u16,
    at: Instant,
    count: u8,
    moved: bool,
    held: bool,
}

pub struct App {
    pub origin: Option<PathBuf>,
    pub fatal: Option<String>,
    pub auth: Option<AuthStatus>,
    pub auth_error: Option<String>,
    pub auth_inflight: bool,
    pub namespaces: Vec<Namespace>,
    pub repos: Vec<Repo>,
    pub list_error: Option<String>,
    pub list_inflight: bool,
    pub selected: Option<usize>,
    pub focus: Focus,
    pub filter: Editor,
    pub list_offset: usize,
    pub list_height: usize,
    pub detail_scroll: usize,
    pub detail_height: usize,
    pub detail_lines: usize,
    pub value_width: usize,
    pub selected_field: FieldId,
    pub cursor: DetailCursor,
    pub selected_branch: Option<String>,
    branch_locked: bool,
    pub detail_inflight: bool,
    pub pulls_inflight: bool,
    pub branches_inflight: bool,
    pub commits_inflight: bool,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    pub selection: Option<Selection>,
    pub screen: Screen,
    list_generation: u64,
    detail_generation: u64,
    create_generation: u64,
    edit_generation: u64,
    commits_generation: u64,
    pending_select: Option<String>,
    press: Option<Press>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            origin: None,
            fatal: None,
            auth: None,
            auth_error: None,
            auth_inflight: false,
            namespaces: Vec::new(),
            repos: Vec::new(),
            list_error: None,
            list_inflight: false,
            selected: None,
            focus: Focus::List,
            filter: Editor::new(""),
            list_offset: 0,
            list_height: 0,
            detail_scroll: 0,
            detail_height: 0,
            detail_lines: 0,
            value_width: 0,
            selected_field: FieldId::Clone,
            cursor: DetailCursor::Field,
            selected_branch: None,
            branch_locked: false,
            detail_inflight: false,
            pulls_inflight: false,
            branches_inflight: false,
            commits_inflight: false,
            overlay: None,
            toast: None,
            selection: None,
            screen: Screen::default(),
            list_generation: 0,
            detail_generation: 0,
            create_generation: 0,
            edit_generation: 0,
            commits_generation: 0,
            pending_select: None,
            press: None,
        }
    }
}

impl App {
    pub fn boot() -> (Self, Vec<Job>) {
        let mut app = Self::default();
        match origin::resolve() {
            Ok(bin) => {
                app.origin = Some(bin);
                let jobs = app.refresh_jobs();
                (app, jobs)
            }
            Err(err) => {
                app.fatal = Some(err);
                (app, Vec::new())
            }
        }
    }

    pub fn adopt(&mut self, screen: Screen) {
        self.value_width = screen.value_width;
        self.detail_height = screen.detail_height;
        self.detail_lines = screen.detail_lines;
        self.list_height = screen.list_height;
        self.list_offset = screen.list_offset;
        self.detail_scroll = screen.detail_scroll;
        self.screen = screen;
    }

    pub fn poll_timeout(&self) -> Duration {
        let idle = Duration::from_millis(50);
        match &self.toast {
            Some(toast) => {
                let now = Instant::now();
                if toast.until <= now {
                    Duration::from_millis(1)
                } else {
                    (toast.until - now).min(idle)
                }
            }
            None => idle,
        }
    }

    pub fn tick(&mut self) {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.until <= Instant::now())
        {
            self.toast = None;
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        let millis = match kind {
            ToastKind::Info => 1600,
            ToastKind::Err => 4200,
        };
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_millis(millis),
        });
    }

    pub fn selected_repo(&self) -> Option<&Repo> {
        self.selected.and_then(|index| self.repos.get(index))
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let query = self.filter.value.trim().to_lowercase();
        self.repos
            .iter()
            .enumerate()
            .filter_map(|(index, repo)| {
                if query.is_empty()
                    || repo.full_name.to_lowercase().contains(&query)
                    || repo.name.to_lowercase().contains(&query)
                {
                    Some(index)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn grouped(&self) -> bool {
        let mut owners = Vec::new();
        for repo in &self.repos {
            if !owners.iter().any(|owner: &String| owner == &repo.owner) {
                owners.push(repo.owner.clone());
            }
        }
        owners.len() > 1
    }

    pub fn rows(&self) -> Vec<DisplayRow> {
        display_rows(&self.repos, &self.visible_indices(), self.grouped())
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Effect {
        if key.kind == KeyEventKind::Release {
            return Effect::None;
        }
        self.selection = None;
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Quit;
        }
        if self.fatal.is_some() {
            return if key.code == KeyCode::Char('q') {
                Effect::Quit
            } else {
                Effect::None
            };
        }
        if self.overlay.is_some() {
            return self.overlay_key(key);
        }
        match self.focus {
            Focus::Filter => self.filter_key(key),
            Focus::List => self.list_key(key),
            Focus::Detail => self.detail_key(key),
        }
    }

    pub fn on_paste(&mut self, text: String) -> Effect {
        let truncated = text.chars().count() > 300;
        if let Some(Overlay::Edit(form)) = &mut self.overlay {
            if let EditKind::Branch(editor) = &mut form.kind {
                editor.insert_str(&text);
                if truncated {
                    self.toast("Paste truncated", ToastKind::Info);
                }
            }
            return Effect::None;
        }
        if let Some(Overlay::Create(form)) = &mut self.overlay {
            form.focused_editor().insert_str(&text);
            if truncated {
                self.toast("Paste truncated", ToastKind::Info);
            }
            return Effect::None;
        }
        if self.focus == Focus::Filter {
            self.filter.insert_str(&text);
            let jobs = self.revalidate_selection();
            if truncated {
                self.toast("Paste truncated", ToastKind::Info);
            }
            return jobs_effect(jobs);
        }
        Effect::None
    }

    pub fn on_mouse(&mut self, event: MouseEvent, buf: &Buffer) -> Effect {
        let (x, y) = (event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let count = match &self.press {
                    Some(press)
                        if !press.held
                            && press.x == x
                            && press.y == y
                            && press.at.elapsed() < Duration::from_millis(400) =>
                    {
                        press.count.saturating_add(1)
                    }
                    _ => 1,
                };
                self.press = Some(Press {
                    x,
                    y,
                    at: Instant::now(),
                    count,
                    moved: false,
                    held: true,
                });
                self.selection = None;
                Effect::None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.overlay.is_some() {
                    return Effect::None;
                }
                if let Some(press) = &mut self.press
                    && press.held
                    && (x, y) != (press.x, press.y)
                {
                    press.moved = true;
                    self.selection = Some(Selection::new((press.x, press.y), (x, y)));
                }
                Effect::None
            }
            MouseEventKind::Up(MouseButton::Left) => self.mouse_up(x, y, buf),
            MouseEventKind::ScrollUp => self.scroll_at(x, y, -1),
            MouseEventKind::ScrollDown => self.scroll_at(x, y, 1),
            _ => Effect::None,
        }
    }

    pub fn on_msg(&mut self, msg: Msg) -> Vec<Job> {
        match msg {
            Msg::Auth(result) => {
                self.auth_inflight = false;
                match result {
                    Ok(status) => {
                        self.auth = Some(status);
                        self.auth_error = None;
                    }
                    Err(err) => self.auth_error = Some(err),
                }
                Vec::new()
            }
            Msg::Namespaces(result) => {
                if let Ok(namespaces) = result {
                    self.namespaces = namespaces;
                }
                Vec::new()
            }
            Msg::RepoList { generation, result } => {
                if generation != self.list_generation {
                    return Vec::new();
                }
                self.list_inflight = false;
                match result {
                    Ok(listed) => {
                        self.list_error = None;
                        let old = std::mem::take(&mut self.repos);
                        let previous = self
                            .selected
                            .and_then(|index| old.get(index).map(|repo| repo.full_name.clone()));
                        self.repos = listed
                            .into_iter()
                            .map(|item| {
                                let mut repo = Repo::from_listed(item);
                                if let Some(prev) =
                                    old.iter().find(|prev| prev.full_name == repo.full_name)
                                {
                                    repo.summary = prev.summary.clone();
                                    repo.fetched = prev.fetched.clone();
                                    repo.view = prev.view.clone();
                                    repo.pulls = prev.pulls.clone();
                                    repo.pulls_error = prev.pulls_error.clone();
                                    repo.branches = prev.branches.clone();
                                    repo.branches_error = prev.branches_error.clone();
                                    repo.commits = prev.commits.clone();
                                    repo.commits_error = prev.commits_error.clone();
                                    repo.commits_for = prev.commits_for.clone();
                                }
                                repo
                            })
                            .collect();
                        let mut jobs = self.enrich_jobs();
                        jobs.extend(self.finish_selection(previous));
                        jobs
                    }
                    Err(err) => {
                        if self.repos.is_empty() {
                            self.list_error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
            Msg::Enrich { owner, result } => {
                if let Ok(apis) = result {
                    for api in apis {
                        if let Some(repo) = self
                            .repos
                            .iter_mut()
                            .find(|repo| repo.owner == owner && repo.full_name == api.full_name)
                        {
                            repo.apply_summary(api);
                        }
                    }
                }
                Vec::new()
            }
            Msg::Detail {
                generation,
                full_name,
                view,
                api,
            } => {
                if generation != self.detail_generation {
                    return Vec::new();
                }
                self.detail_inflight = false;
                if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                {
                    repo.apply_detail(*view, *api);
                    let default_name = repo.default_branch_name();
                    order_branches(&mut repo.branches, default_name.as_deref());
                }
                if self.selected_name().as_deref() == Some(full_name.as_str()) {
                    self.place_branch_cursor();
                    self.sync_branch(false)
                } else {
                    Vec::new()
                }
            }
            Msg::Branches {
                generation,
                full_name,
                result,
            } => {
                if generation != self.detail_generation {
                    return Vec::new();
                }
                self.branches_inflight = false;
                let selected = self.selected_name().as_deref() == Some(full_name.as_str());
                let loaded = if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                {
                    match result {
                        Ok(branches) => {
                            repo.branches = branches;
                            repo.branches_error = None;
                            let default_name = repo.default_branch_name();
                            order_branches(&mut repo.branches, default_name.as_deref());
                            true
                        }
                        Err(err) => {
                            repo.branches_error = Some(err);
                            false
                        }
                    }
                } else {
                    false
                };
                if selected && loaded {
                    self.place_branch_cursor();
                    self.sync_branch(true)
                } else {
                    Vec::new()
                }
            }
            Msg::Commits {
                generation,
                full_name,
                branch,
                result,
            } => {
                if generation != self.commits_generation
                    || self.selected_branch.as_deref() != Some(branch.as_str())
                    || self.selected_name().as_deref() != Some(full_name.as_str())
                {
                    return Vec::new();
                }
                self.commits_inflight = false;
                if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                {
                    match result {
                        Ok(commits) => {
                            repo.commits = commits;
                            repo.commits_for = Some(branch);
                            repo.commits_error = None;
                        }
                        Err(err) => {
                            if repo.commits_for.as_deref() != Some(branch.as_str()) {
                                repo.commits.clear();
                            }
                            repo.commits_for = Some(branch);
                            repo.commits_error = Some(err);
                        }
                    }
                }
                self.clamp_commit();
                Vec::new()
            }
            Msg::Pulls {
                generation,
                full_name,
                result,
            } => {
                if generation != self.detail_generation {
                    return Vec::new();
                }
                self.pulls_inflight = false;
                if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                {
                    match result {
                        Ok(pulls) => {
                            repo.pulls = pulls;
                            repo.pulls_error = None;
                        }
                        Err(err) => repo.pulls_error = Some(err),
                    }
                }
                self.clamp_pull();
                Vec::new()
            }
            Msg::Updated {
                generation,
                full_name,
                result,
            } => {
                if generation != self.edit_generation {
                    return Vec::new();
                }
                match *result {
                    Ok(api) => {
                        if let Some(repo) = self
                            .repos
                            .iter_mut()
                            .find(|repo| repo.full_name == full_name)
                        {
                            repo.apply_settings(api);
                        }
                        if matches!(
                            self.overlay,
                            Some(Overlay::Edit(ref form)) if form.generation == generation
                        ) {
                            self.overlay = None;
                        }
                        self.toast("Saved repository settings", ToastKind::Info);
                        Vec::new()
                    }
                    Err(err) => {
                        if let Some(Overlay::Edit(form)) = &mut self.overlay
                            && form.generation == generation
                        {
                            form.busy = false;
                            form.error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
            Msg::Created {
                generation,
                full_name,
                result,
            } => {
                if generation != self.create_generation {
                    return Vec::new();
                }
                match result {
                    Ok(()) => {
                        self.overlay = None;
                        self.pending_select = Some(full_name);
                        self.focus = Focus::List;
                        self.toast("Repository created", ToastKind::Info);
                        vec![self.request_list()]
                    }
                    Err(err) => {
                        if let Some(Overlay::Create(form)) = &mut self.overlay {
                            form.busy = false;
                            form.error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
        }
    }

    fn mouse_up(&mut self, x: u16, y: u16, buf: &Buffer) -> Effect {
        let Some(press) = self.press.as_mut() else {
            return Effect::None;
        };
        if !press.held {
            return Effect::None;
        }
        press.held = false;
        if (x, y) != (press.x, press.y) {
            press.moved = true;
            self.selection = Some(Selection::new((press.x, press.y), (x, y)));
        }
        let moved = press.moved;
        let count = press.count;
        let anchor = (press.x, press.y);
        if moved {
            self.press = None;
            if self.overlay.is_some() {
                self.selection = None;
                return Effect::None;
            }
            if let Some(selection) = self.selection {
                let text = selection::extract(buf, selection, &self.screen.continues);
                if text.is_empty() {
                    self.selection = None;
                    return Effect::None;
                }
                return Effect::Copy {
                    text,
                    toast: "Copied".to_string(),
                };
            }
            return Effect::None;
        }
        self.on_click(anchor.0, anchor.1, count, buf)
    }

    fn on_click(&mut self, x: u16, y: u16, count: u8, buf: &Buffer) -> Effect {
        if self.overlay.is_some() {
            let help = matches!(self.overlay, Some(Overlay::Help));
            let inside = self
                .screen
                .hits
                .modal
                .is_some_and(|rect| contains(rect, x, y));
            if !inside {
                if help {
                    self.overlay = None;
                }
                return Effect::None;
            }
            if let Some(index) = self
                .screen
                .hits
                .modal_inputs
                .iter()
                .position(|rect| contains(*rect, x, y))
            {
                match &mut self.overlay {
                    Some(Overlay::Create(form)) => form.focus = index,
                    Some(Overlay::Edit(form)) => {
                        if let EditKind::Visibility { choice, .. } = &mut form.kind
                            && index < VISIBILITIES.len()
                        {
                            *choice = index;
                        }
                    }
                    _ => {}
                }
            }
            return Effect::None;
        }

        let new_repo = self.screen.hits.new_repo;
        let refresh = self.screen.hits.refresh;
        let copy_clone = self.screen.hits.copy_clone;
        let copy_web = self.screen.hits.copy_web;
        let filter = self.screen.hits.filter;
        let list_area = self.screen.hits.list_area;
        let detail_area = self.screen.hits.detail_area;
        let list_rows = self.screen.hits.list_rows.clone();
        let branch_rows = self.screen.hits.branch_rows.clone();
        let commit_rows = self.screen.hits.commit_rows.clone();
        let pull_rows = self.screen.hits.pull_rows.clone();
        let fields = self.screen.hits.fields.clone();
        let continues = self.screen.continues.clone();

        if new_repo.is_some_and(|rect| contains(rect, x, y)) {
            self.open_create();
            return Effect::None;
        }
        if refresh.is_some_and(|rect| contains(rect, x, y)) {
            return Effect::Jobs(self.refresh_jobs());
        }
        if copy_clone.is_some_and(|rect| contains(rect, x, y)) {
            return self.copy_clone();
        }
        if copy_web.is_some_and(|rect| contains(rect, x, y)) {
            return self.copy_web();
        }
        if count >= 2
            && let Some(id) = fields
                .iter()
                .find(|hit| contains(hit.rect, x, y))
                .map(|hit| hit.id)
        {
            self.highlight_field_rects(&fields, id);
            self.focus = Focus::Detail;
            self.cursor = DetailCursor::Field;
            self.selected_field = id;
            return self.copy_field(id);
        }
        if count >= 2
            && let Some(index) = crate::screen::hit(&branch_rows, x, y)
        {
            self.focus = Focus::Detail;
            self.cursor = DetailCursor::Branch(index);
            self.branch_locked = true;
            if let Some(name) = self.branch_name(index) {
                self.selected_branch = Some(name);
            }
            return self.copy_branch(index);
        }
        if count >= 2
            && let Some(index) = crate::screen::hit(&commit_rows, x, y)
        {
            self.focus = Focus::Detail;
            self.cursor = DetailCursor::Commit(index);
            return self.copy_commit(index);
        }
        if count >= 2
            && let Some(index) = crate::screen::hit(&pull_rows, x, y)
        {
            self.focus = Focus::Detail;
            self.cursor = DetailCursor::Pull(index);
            return self.copy_pull(index);
        }
        if count >= 3
            && let Some(selection) = selection::line_span(buf, y)
        {
            self.selection = Some(selection);
            let text = selection::extract(buf, selection, &continues);
            if !text.is_empty() {
                return Effect::Copy {
                    text,
                    toast: "Copied".to_string(),
                };
            }
        } else if count == 2
            && let Some(selection) = selection::word_span(buf, x, y)
        {
            self.selection = Some(selection);
            let text = selection::extract(buf, selection, &continues);
            if !text.is_empty() {
                return Effect::Copy {
                    text,
                    toast: "Copied".to_string(),
                };
            }
        }

        if filter.is_some_and(|rect| contains(rect, x, y)) {
            self.focus = Focus::Filter;
            return Effect::None;
        }
        if let Some(index) = crate::screen::hit(&list_rows, x, y) {
            self.focus = Focus::List;
            return jobs_effect(self.select_index(Some(index), self.selected_name(), false));
        }
        if let Some(id) = fields
            .iter()
            .find(|hit| contains(hit.rect, x, y))
            .map(|hit| hit.id)
        {
            self.focus = Focus::Detail;
            self.cursor = DetailCursor::Field;
            self.selected_field = id;
            self.ensure_detail_visible();
            return Effect::None;
        }
        if let Some(index) = crate::screen::hit(&branch_rows, x, y) {
            self.focus = Focus::Detail;
            return jobs_effect(self.apply_item(DetailTarget::Branch(index)));
        }
        if let Some(index) = crate::screen::hit(&commit_rows, x, y) {
            self.focus = Focus::Detail;
            return jobs_effect(self.apply_item(DetailTarget::Commit(index)));
        }
        if let Some(index) = crate::screen::hit(&pull_rows, x, y) {
            self.focus = Focus::Detail;
            return jobs_effect(self.apply_item(DetailTarget::Pull(index)));
        }
        if detail_area.is_some_and(|rect| contains(rect, x, y)) {
            self.focus_detail();
        } else if list_area.is_some_and(|rect| contains(rect, x, y)) {
            self.focus = Focus::List;
        }
        Effect::None
    }

    fn highlight_field_rects(&mut self, fields: &[FieldHit], id: FieldId) {
        let rects: Vec<_> = fields
            .iter()
            .filter(|hit| hit.id == id)
            .map(|hit| hit.rect)
            .collect();
        if let (Some(first), Some(last)) = (rects.first(), rects.last())
            && last.width > 0
        {
            self.selection = Some(Selection::new(
                (first.x, first.y),
                (last.x + last.width - 1, last.y),
            ));
        }
    }

    fn scroll_at(&mut self, x: u16, y: u16, delta: isize) -> Effect {
        if self.overlay.is_some() {
            return Effect::None;
        }
        if self
            .screen
            .hits
            .detail_area
            .is_some_and(|rect| contains(rect, x, y))
        {
            self.scroll_detail(delta);
            Effect::None
        } else if self
            .screen
            .hits
            .list_area
            .is_some_and(|rect| contains(rect, x, y))
        {
            jobs_effect(self.move_repo(delta))
        } else {
            Effect::None
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) -> Effect {
        let help = matches!(self.overlay, Some(Overlay::Help));
        let busy = matches!(self.overlay, Some(Overlay::Create(ref form)) if form.busy);
        if help {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') => {
                    self.overlay = None;
                    Effect::None
                }
                KeyCode::Char('q') => Effect::Quit,
                _ => Effect::None,
            };
        }
        if matches!(self.overlay, Some(Overlay::Edit(_))) {
            return self.edit_overlay_key(key);
        }
        if !matches!(self.overlay, Some(Overlay::Create(_))) {
            return Effect::None;
        }
        if busy {
            self.toast("Still creating the repository", ToastKind::Info);
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_create(),
            KeyCode::Tab | KeyCode::Down => {
                self.cycle_form(1);
                Effect::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.cycle_form(-1);
                Effect::None
            }
            _ => {
                if let Some(Overlay::Create(form)) = &mut self.overlay {
                    edit_key(form.focused_editor(), &key);
                }
                Effect::None
            }
        }
    }

    fn filter_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc => {
                self.filter = Editor::new("");
                self.focus = Focus::List;
                jobs_effect(self.revalidate_selection())
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::List;
                Effect::None
            }
            KeyCode::Up => jobs_effect(self.move_repo(-1)),
            KeyCode::Down => jobs_effect(self.move_repo(1)),
            _ => {
                let before = self.filter.value.clone();
                if edit_key(&mut self.filter, &key) && self.filter.value != before {
                    jobs_effect(self.revalidate_selection())
                } else {
                    Effect::None
                }
            }
        }
    }

    fn list_key(&mut self, key: KeyEvent) -> Effect {
        if let Some(effect) = self.global_key(&key) {
            return effect;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => jobs_effect(self.move_repo(-1)),
            KeyCode::Down | KeyCode::Char('j') => jobs_effect(self.move_repo(1)),
            KeyCode::Home | KeyCode::Char('g') => jobs_effect(self.jump_repo(false)),
            KeyCode::End | KeyCode::Char('G') => jobs_effect(self.jump_repo(true)),
            KeyCode::PageUp => jobs_effect(self.page_repo(false)),
            KeyCode::PageDown => jobs_effect(self.page_repo(true)),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                jobs_effect(self.page_repo(false))
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                jobs_effect(self.page_repo(true))
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab | KeyCode::BackTab => {
                self.focus_detail();
                Effect::None
            }
            KeyCode::Char('y') | KeyCode::Enter => self.copy_focused(),
            _ => Effect::None,
        }
    }

    fn detail_key(&mut self, key: KeyEvent) -> Effect {
        if let Some(effect) = self.global_key(&key) {
            return effect;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => jobs_effect(self.move_field(-1)),
            KeyCode::Down | KeyCode::Char('j') => jobs_effect(self.move_field(1)),
            KeyCode::Home | KeyCode::Char('g') => jobs_effect(self.move_field(-1000)),
            KeyCode::End | KeyCode::Char('G') => jobs_effect(self.move_field(1000)),
            KeyCode::PageUp => jobs_effect(self.move_field(-4)),
            KeyCode::PageDown => jobs_effect(self.move_field(4)),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::List;
                Effect::None
            }
            KeyCode::Char('y') | KeyCode::Enter => self.copy_focused(),
            KeyCode::Char('e')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.begin_edit()
            }
            _ => Effect::None,
        }
    }

    fn global_key(&mut self, key: &KeyEvent) -> Option<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Char('/') => {
                self.focus = Focus::Filter;
                Some(Effect::None)
            }
            KeyCode::Char('n') => {
                self.open_create();
                Some(Effect::None)
            }
            KeyCode::Char('r') => Some(Effect::Jobs(self.refresh_jobs())),
            KeyCode::Char('w') => Some(self.copy_web()),
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                Some(Effect::None)
            }
            KeyCode::Char('q') => Some(Effect::Quit),
            _ => None,
        }
    }

    fn copy_focused(&mut self) -> Effect {
        if self.focus != Focus::Detail {
            return self.copy_clone();
        }
        match self.cursor {
            DetailCursor::Branch(index) => self.copy_branch(index),
            DetailCursor::Commit(index) => self.copy_commit(index),
            DetailCursor::Pull(index) => self.copy_pull(index),
            DetailCursor::Field => self.copy_field(self.selected_field),
        }
    }

    fn copy_branch(&mut self, index: usize) -> Effect {
        match self.branch_name(index) {
            Some(name) if !name.is_empty() => Effect::Copy {
                text: name,
                toast: "Copied branch".to_string(),
            },
            _ => {
                self.toast("Nothing to copy", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn copy_commit(&mut self, index: usize) -> Effect {
        match self
            .selected_repo()
            .and_then(|repo| repo.commits.get(index))
            .map(|commit| commit.sha.clone())
        {
            Some(sha) if !sha.is_empty() => Effect::Copy {
                text: sha,
                toast: "Copied commit".to_string(),
            },
            _ => {
                self.toast("Nothing to copy", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn copy_pull(&mut self, index: usize) -> Effect {
        match self
            .selected_repo()
            .and_then(|repo| repo.pulls.get(index))
            .map(|pull| pull.url.clone())
        {
            Some(url) if !url.is_empty() => Effect::Copy {
                text: url,
                toast: "Copied pull request".to_string(),
            },
            _ => {
                self.toast("Nothing to copy", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn copy_clone(&mut self) -> Effect {
        match self
            .selected_repo()
            .and_then(|repo| repo.clone_url().map(str::to_string))
        {
            Some(url) => Effect::Copy {
                text: url,
                toast: "Copied clone URL".to_string(),
            },
            None => {
                self.toast("No clone URL", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn copy_web(&mut self) -> Effect {
        match self
            .selected_repo()
            .and_then(|repo| repo.web_url().map(str::to_string))
        {
            Some(url) => Effect::Copy {
                text: url,
                toast: "Copied web URL".to_string(),
            },
            None => {
                self.toast("No web URL yet", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn copy_field(&mut self, id: FieldId) -> Effect {
        let found = self.selected_repo().and_then(|repo| {
            repo.fields()
                .into_iter()
                .find(|field| field.id == id)
                .map(|field| (field.copy, field.label))
        });
        match found {
            Some((text, label)) if !text.is_empty() => Effect::Copy {
                text,
                toast: format!("Copied {label}"),
            },
            _ => {
                self.toast("Nothing to copy", ToastKind::Err);
                Effect::None
            }
        }
    }

    fn focus_detail(&mut self) {
        self.focus = Focus::Detail;
        if self.cursor_in_range() && !matches!(self.cursor, DetailCursor::Field) {
            self.ensure_detail_visible();
            return;
        }
        self.cursor = DetailCursor::Field;
        let ids = self.field_ids();
        if !ids.contains(&self.selected_field) {
            self.selected_field = if ids.contains(&FieldId::Clone) {
                FieldId::Clone
            } else {
                ids.first().copied().unwrap_or(FieldId::Name)
            };
        }
        self.ensure_detail_visible();
    }

    fn cursor_in_range(&self) -> bool {
        let Some(repo) = self.selected_repo() else {
            return false;
        };
        match self.cursor {
            DetailCursor::Field => true,
            DetailCursor::Branch(index) => repo.branches.len() > 1 && index < repo.branches.len(),
            DetailCursor::Commit(index) => index < repo.commits.len(),
            DetailCursor::Pull(index) => index < repo.pulls.len(),
        }
    }

    fn field_ids(&self) -> Vec<FieldId> {
        self.selected_repo()
            .map(|repo| repo.fields().iter().map(|field| field.id).collect())
            .unwrap_or_default()
    }

    fn move_field(&mut self, delta: isize) -> Vec<Job> {
        let items = self.detail_items();
        if items.is_empty() {
            return Vec::new();
        }
        let current = self.item_position(&items);
        let next = (current as isize + delta).clamp(0, items.len() as isize - 1) as usize;
        self.apply_item(items[next])
    }

    fn detail_items(&self) -> Vec<DetailTarget> {
        let mut items: Vec<_> = self
            .field_ids()
            .into_iter()
            .map(DetailTarget::Field)
            .collect();
        let Some(repo) = self.selected_repo() else {
            return items;
        };
        if repo.branches.len() > 1 {
            for index in 0..repo.branches.len() {
                items.push(DetailTarget::Branch(index));
            }
        }
        for index in 0..repo.commits.len() {
            items.push(DetailTarget::Commit(index));
        }
        for index in 0..repo.pulls.len() {
            items.push(DetailTarget::Pull(index));
        }
        items
    }

    fn item_position(&self, items: &[DetailTarget]) -> usize {
        let wanted = self.cursor_target();
        items.iter().position(|item| *item == wanted).unwrap_or(0)
    }

    fn cursor_target(&self) -> DetailTarget {
        match self.cursor {
            DetailCursor::Field => DetailTarget::Field(self.selected_field),
            DetailCursor::Branch(index) => DetailTarget::Branch(index),
            DetailCursor::Commit(index) => DetailTarget::Commit(index),
            DetailCursor::Pull(index) => DetailTarget::Pull(index),
        }
    }

    fn apply_item(&mut self, item: DetailTarget) -> Vec<Job> {
        let jobs = match item {
            DetailTarget::Field(id) => {
                self.cursor = DetailCursor::Field;
                self.selected_field = id;
                Vec::new()
            }
            DetailTarget::Branch(index) => self.focus_branch(index),
            DetailTarget::Commit(index) => {
                self.cursor = DetailCursor::Commit(index);
                Vec::new()
            }
            DetailTarget::Pull(index) => {
                self.cursor = DetailCursor::Pull(index);
                Vec::new()
            }
        };
        self.ensure_detail_visible();
        jobs
    }

    fn focus_branch(&mut self, index: usize) -> Vec<Job> {
        self.cursor = DetailCursor::Branch(index);
        self.branch_locked = true;
        let Some(name) = self.branch_name(index) else {
            return Vec::new();
        };
        self.select_branch(name, false)
    }

    fn branch_name(&self, index: usize) -> Option<String> {
        self.selected_repo()
            .and_then(|repo| repo.branches.get(index).map(|branch| branch.name.clone()))
    }

    fn active_spans(&self) -> Vec<DetailSpan> {
        if !self.screen.spans.is_empty() {
            return self.screen.spans.clone();
        }
        let Some(repo) = self.selected_repo() else {
            return Vec::new();
        };
        spans_from(&detail_lines(
            repo,
            self.value_width,
            self.pulls_inflight,
            self.branches_inflight,
            self.commits_inflight,
        ))
    }

    fn ensure_detail_visible(&mut self) {
        let height = self.detail_height;
        if height == 0 {
            return;
        }
        let wanted = self.cursor_target();
        let Some(span) = self
            .active_spans()
            .into_iter()
            .find(|span| span.target == wanted)
        else {
            return;
        };
        if span.start < self.detail_scroll {
            self.detail_scroll = span.start;
        } else if span.end > self.detail_scroll + height {
            self.detail_scroll = span.end.saturating_sub(height);
        }
    }

    fn clamp_pull(&mut self) {
        let len = self
            .selected_repo()
            .map(|repo| repo.pulls.len())
            .unwrap_or(0);
        if matches!(self.cursor, DetailCursor::Pull(index) if index >= len) {
            self.cursor = DetailCursor::Field;
        }
    }

    fn clamp_commit(&mut self) {
        let len = self
            .selected_repo()
            .map(|repo| repo.commits.len())
            .unwrap_or(0);
        if matches!(self.cursor, DetailCursor::Commit(index) if index >= len) {
            self.cursor = DetailCursor::Field;
        }
    }

    fn scroll_detail(&mut self, delta: isize) {
        let height = self.detail_height;
        if height == 0 {
            return;
        }
        let max = self.detail_lines.saturating_sub(height);
        let next = self.detail_scroll as isize + delta;
        self.detail_scroll = next.clamp(0, max as isize) as usize;
    }

    fn move_repo(&mut self, delta: isize) -> Vec<Job> {
        let rows = self.rows();
        let positions = repo_positions(&rows);
        if positions.is_empty() {
            return self.select_index(None, self.selected_name(), false);
        }
        let current = self.selected.and_then(|selected| {
            positions.iter().position(
                |pos| matches!(rows[*pos].kind, RowKind::Repo(index) if index == selected),
            )
        });
        let next = match current {
            Some(index) => (index as isize + delta).clamp(0, positions.len() as isize - 1) as usize,
            None if delta < 0 => positions.len() - 1,
            None => 0,
        };
        let RowKind::Repo(index) = rows[positions[next]].kind else {
            return Vec::new();
        };
        self.select_index(Some(index), self.selected_name(), false)
    }

    fn jump_repo(&mut self, to_end: bool) -> Vec<Job> {
        let rows = self.rows();
        let positions = repo_positions(&rows);
        if positions.is_empty() {
            return self.select_index(None, self.selected_name(), false);
        }
        let pos = if to_end { positions.len() - 1 } else { 0 };
        let RowKind::Repo(index) = rows[positions[pos]].kind else {
            return Vec::new();
        };
        self.select_index(Some(index), self.selected_name(), false)
    }

    fn page_repo(&mut self, down: bool) -> Vec<Job> {
        let step = (self.list_height as isize).saturating_sub(1).max(1);
        self.move_repo(if down { step } else { -step })
    }

    fn revalidate_selection(&mut self) -> Vec<Job> {
        let visible = self.visible_indices();
        let previous = self.selected_name();
        if let Some(selected) = self.selected
            && visible.contains(&selected)
        {
            self.ensure_list_visible();
            return Vec::new();
        }
        if visible.is_empty() {
            return self.select_index(None, previous, false);
        }
        self.select_index(visible.first().copied(), previous, true)
    }

    fn finish_selection(&mut self, previous: Option<String>) -> Vec<Job> {
        if let Some(want) = self.pending_select.clone()
            && let Some(index) = self.find_pending(&want)
        {
            self.pending_select = None;
            return self.select_index(Some(index), previous, true);
        }
        if let Some(previous_name) = &previous
            && let Some(index) = self
                .repos
                .iter()
                .position(|repo| &repo.full_name == previous_name)
        {
            return self.select_index(Some(index), previous, true);
        }
        let first = self.visible_indices().first().copied();
        self.select_index(first, previous, true)
    }

    fn find_pending(&self, want: &str) -> Option<usize> {
        if want.contains('/') {
            self.repos.iter().position(|repo| repo.full_name == want)
        } else {
            self.repos.iter().position(|repo| repo.name == want)
        }
    }

    fn select_index(
        &mut self,
        index: Option<usize>,
        previous: Option<String>,
        refresh: bool,
    ) -> Vec<Job> {
        self.selected = index;
        self.ensure_list_visible();
        let next = self.selected_name();
        if next.is_none() {
            self.detail_inflight = false;
            self.pulls_inflight = false;
            self.branches_inflight = false;
            self.commits_inflight = false;
            return Vec::new();
        }
        if previous != next {
            self.detail_scroll = 0;
            self.selected_field = FieldId::Clone;
            self.cursor = DetailCursor::Field;
            self.selected_branch = None;
            self.branch_locked = false;
        }
        if previous != next || refresh {
            self.detail_generation += 1;
            self.detail_inflight = true;
            self.pulls_inflight = true;
            self.branches_inflight = true;
            self.commits_inflight = false;
            if let Some(index) = index {
                self.repos[index].detail_error = None;
                self.repos[index].pulls_error = None;
                self.repos[index].branches_error = None;
                self.repos[index].commits_error = None;
            }
            let full_name = next.unwrap();
            return vec![
                Job::Detail {
                    full_name: full_name.clone(),
                    generation: self.detail_generation,
                },
                Job::Branches {
                    full_name: full_name.clone(),
                    generation: self.detail_generation,
                },
                Job::Pulls {
                    full_name,
                    generation: self.detail_generation,
                },
            ];
        }
        Vec::new()
    }

    fn place_branch_cursor(&mut self) {
        if !matches!(self.cursor, DetailCursor::Branch(_)) {
            return;
        }
        let Some(name) = self.selected_branch.clone() else {
            self.cursor = DetailCursor::Field;
            self.branch_locked = false;
            return;
        };
        match self
            .selected_repo()
            .and_then(|repo| repo.branches.iter().position(|branch| branch.name == name))
        {
            Some(index) => self.cursor = DetailCursor::Branch(index),
            None => {
                self.cursor = DetailCursor::Field;
                self.selected_branch = None;
                self.branch_locked = false;
            }
        }
    }

    fn sync_branch(&mut self, force: bool) -> Vec<Job> {
        let locked = if self.branch_locked {
            self.selected_branch.clone().filter(|name| {
                self.selected_repo()
                    .is_some_and(|repo| repo.branches.iter().any(|branch| &branch.name == name))
            })
        } else {
            None
        };
        let Some(name) = locked.or_else(|| self.selected_repo().and_then(preferred_branch)) else {
            return Vec::new();
        };
        self.select_branch(name, force)
    }

    fn select_branch(&mut self, name: String, force: bool) -> Vec<Job> {
        let already = self.selected_branch.as_deref() == Some(name.as_str());
        self.selected_branch = Some(name.clone());
        let loaded = self.selected_repo().is_some_and(|repo| {
            repo.commits_for.as_deref() == Some(name.as_str()) && repo.commits_error.is_none()
        });
        if !force && (loaded && !self.commits_inflight || already && self.commits_inflight) {
            return Vec::new();
        }
        let Some(full_name) = self.selected_name() else {
            return Vec::new();
        };
        self.commits_generation += 1;
        let generation = self.commits_generation;
        self.commits_inflight = true;
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|repo| repo.full_name == full_name)
        {
            if repo.commits_for.as_deref() != Some(name.as_str()) {
                repo.commits.clear();
            }
            repo.commits_error = None;
        }
        self.clamp_commit();
        vec![Job::Commits {
            full_name,
            branch: name,
            generation,
        }]
    }

    fn selected_name(&self) -> Option<String> {
        self.selected_repo().map(|repo| repo.full_name.clone())
    }

    fn ensure_list_visible(&mut self) {
        let height = self.list_height;
        if height == 0 {
            return;
        }
        let rows = self.rows();
        let Some(pos) = self.selected.and_then(|selected| {
            rows.iter()
                .position(|row| matches!(row.kind, RowKind::Repo(index) if index == selected))
        }) else {
            return;
        };
        if pos < self.list_offset {
            self.list_offset = pos;
        } else if pos >= self.list_offset + height {
            self.list_offset = pos + 1 - height;
        }
    }

    fn enrich_jobs(&self) -> Vec<Job> {
        let mut owners = Vec::new();
        for repo in &self.repos {
            if !owners.iter().any(|owner: &String| owner == &repo.owner) {
                owners.push(repo.owner.clone());
            }
        }
        owners
            .into_iter()
            .map(|owner| Job::Enrich { owner })
            .collect()
    }

    fn refresh_jobs(&mut self) -> Vec<Job> {
        self.auth_inflight = true;
        let list = self.request_list();
        vec![Job::Auth, Job::Namespaces, list]
    }

    fn request_list(&mut self) -> Job {
        self.list_generation += 1;
        self.list_inflight = true;
        Job::RepoList {
            generation: self.list_generation,
        }
    }

    fn open_create(&mut self) {
        self.create_generation += 1;
        let namespace = self.default_namespace();
        self.overlay = Some(Overlay::Create(CreateForm {
            name: Editor::new(""),
            namespace: Editor::new(namespace),
            branch: Editor::new("main"),
            focus: 0,
            error: None,
            busy: false,
            generation: self.create_generation,
        }));
    }

    fn default_namespace(&self) -> String {
        self.namespaces
            .iter()
            .find(|ns| ns.kind.as_deref() == Some("user") && ns.can_create)
            .or_else(|| self.namespaces.iter().find(|ns| ns.can_create))
            .map(|ns| ns.slug.clone())
            .or_else(|| self.repos.first().map(|repo| repo.owner.clone()))
            .unwrap_or_default()
    }

    fn cycle_form(&mut self, delta: isize) {
        if let Some(Overlay::Create(form)) = &mut self.overlay {
            form.focus = (form.focus as isize + delta).rem_euclid(3) as usize;
        }
    }

    fn submit_create(&mut self) -> Effect {
        let Some(Overlay::Create(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let name = form.name.value.trim().to_string();
        let namespace = form.namespace.value.trim().to_string();
        let branch = form.branch.value.trim().to_string();
        let generation = form.generation;
        if let Err(message) = validate_create(&name, &namespace, &branch) {
            if let Some(Overlay::Create(form)) = &mut self.overlay {
                form.error = Some(message);
            }
            return Effect::None;
        }
        if let Some(Overlay::Create(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::Create {
            generation,
            namespace,
            name,
            branch,
        }])
    }

    fn begin_edit(&mut self) -> Effect {
        if !matches!(self.cursor, DetailCursor::Field) {
            self.toast("This value can't be edited", ToastKind::Info);
            return Effect::None;
        }
        let Some(repo_name) = self.selected_name() else {
            self.toast("This value can't be edited", ToastKind::Info);
            return Effect::None;
        };
        let field = self.selected_field;
        if !field.editable() {
            self.toast("This value can't be edited", ToastKind::Info);
            return Effect::None;
        }
        let Some(kind) = self.edit_kind(field) else {
            return Effect::None;
        };
        self.edit_generation += 1;
        self.overlay = Some(Overlay::Edit(EditForm {
            full_name: repo_name,
            kind,
            error: None,
            busy: false,
            generation: self.edit_generation,
        }));
        Effect::None
    }

    fn edit_kind(&mut self, field: FieldId) -> Option<EditKind> {
        match field {
            FieldId::Visibility => {
                let current = self
                    .selected_repo()
                    .and_then(|repo| repo.api().and_then(|api| api.visibility.clone()));
                let Some(current) = current else {
                    self.toast("Settings are still loading", ToastKind::Info);
                    return None;
                };
                let choice = VISIBILITIES
                    .iter()
                    .position(|item| *item == current)
                    .unwrap_or(0);
                Some(EditKind::Visibility { current, choice })
            }
            FieldId::Branch => {
                let branch = self.selected_repo().and_then(|repo| {
                    repo.view
                        .as_ref()
                        .and_then(|view| view.default_branch.clone())
                        .or_else(|| repo.api().and_then(|api| api.default_branch.clone()))
                });
                let Some(branch) = branch else {
                    self.toast("Settings are still loading", ToastKind::Info);
                    return None;
                };
                Some(EditKind::Branch(Editor::new(branch)))
            }
            FieldId::MergeCommit => self.flag_edit(true),
            FieldId::Squash => self.flag_edit(false),
            FieldId::DeleteBranch => {
                let current = self
                    .selected_repo()
                    .and_then(|repo| repo.api().and_then(|api| api.delete_branch_on_merge));
                let Some(current) = current else {
                    self.toast("Settings are still loading", ToastKind::Info);
                    return None;
                };
                let next = !current;
                Some(EditKind::Confirm {
                    label: "delete branch on merge",
                    from: current,
                    to: next,
                    patch: RepoPatch::DeleteBranch(next),
                })
            }
            _ => None,
        }
    }

    fn flag_edit(&mut self, merge_commit: bool) -> Option<EditKind> {
        let flags = self.selected_repo().and_then(|repo| {
            repo.api().map(|api| {
                if merge_commit {
                    (api.allow_merge_commit, api.allow_squash_merge)
                } else {
                    (api.allow_squash_merge, api.allow_merge_commit)
                }
            })
        });
        let Some((current, other)) = flags else {
            self.toast("Settings are still loading", ToastKind::Info);
            return None;
        };
        let (Some(current), Some(other)) = (current, other) else {
            self.toast("Settings are still loading", ToastKind::Info);
            return None;
        };
        let next = !current;
        if !next && !other {
            self.toast("Keep merge commits or squash merges", ToastKind::Info);
            return None;
        }
        let patch = if merge_commit {
            RepoPatch::Merge {
                allow_merge: next,
                allow_squash: other,
            }
        } else {
            RepoPatch::Merge {
                allow_merge: other,
                allow_squash: next,
            }
        };
        let label = if merge_commit {
            "merge commits"
        } else {
            "squash merges"
        };
        Some(EditKind::Confirm {
            label,
            from: current,
            to: next,
            patch,
        })
    }

    fn edit_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::Edit(ref form)) if form.busy);
        if busy {
            self.toast("Still saving the setting", ToastKind::Info);
            return Effect::None;
        }
        let branch = matches!(
            self.overlay,
            Some(Overlay::Edit(ref form)) if matches!(form.kind, EditKind::Branch(_))
        );
        let visibility = matches!(
            self.overlay,
            Some(Overlay::Edit(ref form)) if matches!(form.kind, EditKind::Visibility { .. })
        );
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_edit(),
            KeyCode::Char('q') if !branch => Effect::Quit,
            KeyCode::Up | KeyCode::Char('k') if visibility => {
                self.cycle_visibility(-1);
                Effect::None
            }
            KeyCode::Down | KeyCode::Char('j') if visibility => {
                self.cycle_visibility(1);
                Effect::None
            }
            _ if branch => {
                if let Some(Overlay::Edit(form)) = &mut self.overlay
                    && let EditKind::Branch(editor) = &mut form.kind
                {
                    edit_key(editor, &key);
                }
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn cycle_visibility(&mut self, delta: isize) {
        if let Some(Overlay::Edit(form)) = &mut self.overlay
            && let EditKind::Visibility { choice, .. } = &mut form.kind
        {
            let len = VISIBILITIES.len() as isize;
            *choice = (*choice as isize + delta).rem_euclid(len) as usize;
        }
    }

    fn submit_edit(&mut self) -> Effect {
        let Some(Overlay::Edit(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let patch = form.patch();
        let full_name = form.full_name.clone();
        let generation = form.generation;
        if let Some(message) = validate_patch(&patch) {
            if let Some(Overlay::Edit(form)) = &mut self.overlay {
                form.error = Some(message);
            }
            return Effect::None;
        }
        if let Some(Overlay::Edit(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::Update {
            full_name,
            generation,
            patch,
        }])
    }
}

fn jobs_effect(jobs: Vec<Job>) -> Effect {
    if jobs.is_empty() {
        Effect::None
    } else {
        Effect::Jobs(jobs)
    }
}

fn repo_positions(rows: &[DisplayRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| match row.kind {
            RowKind::Repo(_) => Some(index),
            RowKind::Header => None,
        })
        .collect()
}

fn validate_create(name: &str, namespace: &str, branch: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a repository name.".to_string());
    }
    if !valid_token(name, false) {
        return Err("Names use letters, numbers, dots, underscores, and hyphens.".to_string());
    }
    if !namespace.is_empty() && !valid_token(namespace, false) {
        return Err("Namespace uses letters, numbers, dots, underscores, and hyphens.".to_string());
    }
    if !branch.is_empty() && !valid_token(branch, true) {
        return Err("Branch names cannot contain spaces.".to_string());
    }
    Ok(())
}

fn validate_patch(patch: &RepoPatch) -> Option<String> {
    match patch {
        RepoPatch::Branch(branch) if branch.is_empty() => Some("Enter a branch name.".to_string()),
        RepoPatch::Branch(branch) if !valid_token(branch, true) => {
            Some("Branch names cannot contain spaces.".to_string())
        }
        RepoPatch::Merge {
            allow_merge: false,
            allow_squash: false,
        } => Some("Keep merge commits or squash merges".to_string()),
        _ => None,
    }
}

fn valid_token(value: &str, slash: bool) -> bool {
    value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') || (slash && ch == '/')
    })
}

fn edit_key(editor: &mut Editor, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char('u') if ctrl => editor.kill_to_start(),
        KeyCode::Char('w') if ctrl => editor.delete_word_back(),
        KeyCode::Char('d') if ctrl => editor.delete_forward(),
        KeyCode::Backspace if alt || ctrl => editor.delete_word_back(),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete_forward(),
        KeyCode::Left => editor.move_left(),
        KeyCode::Right => editor.move_right(),
        KeyCode::Home => editor.move_home(),
        KeyCode::End => editor.move_end(),
        KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => editor.insert_char(ch),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ApiRepo, Branch, Commit, ListedRepo, PullRequest, ViewRepo};

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    fn sample(owner: &str, name: &str) -> Repo {
        Repo::from_listed(ListedRepo {
            owner: owner.to_string(),
            name: name.to_string(),
            clone_url: format!("https://origin.cursor.com/{owner}/{name}.git"),
        })
    }

    fn fixture(repos: Vec<Repo>) -> App {
        let mut app = App::default();
        app.repos = repos;
        app.list_height = 20;
        app.detail_height = 20;
        app.value_width = 40;
        app.selected = if app.repos.is_empty() { None } else { Some(0) };
        app.auth = Some(AuthStatus {
            account: Some("splaice@gmail.com".to_string()),
            method: Some("login".to_string()),
            endpoint: None,
            token: Some("valid".to_string()),
        });
        app
    }

    #[test]
    fn arrows_skip_namespace_headers() {
        let mut app = fixture(vec![
            sample("a", "one"),
            sample("a", "two"),
            sample("b", "three"),
        ]);
        app.on_key(key('j'));
        assert_eq!(app.selected_repo().unwrap().name, "two");
        app.on_key(key('j'));
        assert_eq!(app.selected_repo().unwrap().full_name, "b/three");
        app.on_key(key('k'));
        assert_eq!(app.selected_repo().unwrap().name, "two");
    }

    #[test]
    fn filter_keeps_a_visible_match() {
        let mut app = fixture(vec![sample("a", "one"), sample("b", "three")]);
        app.on_key(key('/'));
        app.on_key(key('t'));
        app.on_key(key('h'));
        assert_eq!(app.selected_repo().unwrap().name, "three");
        assert!(app.rows().iter().all(|row| row.label != "one"));
    }

    #[test]
    fn y_copies_the_clone_url() {
        let mut app = fixture(vec![sample("splaice", "dotfiles")]);
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "https://origin.cursor.com/splaice/dotfiles.git");
                assert_eq!(toast, "Copied clone URL");
            }
            _ => panic!("expected copy"),
        }
    }

    #[test]
    fn create_requires_a_name() {
        let mut app = fixture(vec![sample("splaice", "dotfiles")]);
        app.on_key(key('n'));
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("empty name was submitted"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::Create(form)) => assert!(form.error.is_some()),
            _ => panic!("form closed"),
        }
    }

    #[test]
    fn double_click_copies_the_field_value() {
        let mut app = fixture(vec![sample("splaice", "dotfiles")]);
        app.screen.hits.fields.push(FieldHit {
            id: FieldId::Clone,
            rect: ratatui::layout::Rect {
                x: 1,
                y: 2,
                width: 10,
                height: 2,
            },
        });
        let buf = Buffer::empty(ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        });
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let release = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let _ = app.on_mouse(click, &buf);
        let _ = app.on_mouse(release, &buf);
        let _ = app.on_mouse(click, &buf);
        match app.on_mouse(release, &buf) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "https://origin.cursor.com/splaice/dotfiles.git");
                assert_eq!(toast, "Copied clone");
            }
            _ => panic!("expected copy"),
        }
    }

    fn settings(visibility: &str, merge: bool, squash: bool) -> Repo {
        let mut repo = sample("splaice", "garage");
        repo.fetched = Some(ApiRepo {
            full_name: "splaice/garage".to_string(),
            visibility: Some(visibility.to_string()),
            default_branch: Some("main".to_string()),
            allow_merge_commit: Some(merge),
            allow_squash_merge: Some(squash),
            delete_branch_on_merge: Some(false),
            web_url: Some("https://cursor.com/codebase/splaice/garage".to_string()),
            ..ApiRepo::default()
        });
        repo
    }

    #[test]
    fn e_edits_visibility_and_refuses_the_name() {
        let mut app = fixture(vec![settings("private", true, true)]);
        app.focus = Focus::Detail;
        app.selected_field = FieldId::Name;
        app.on_key(key('e'));
        assert!(app.overlay.is_none());
        app.selected_field = FieldId::Visibility;
        app.on_key(key('e'));
        match &app.overlay {
            Some(Overlay::Edit(form)) => match &form.kind {
                EditKind::Visibility { current, choice } => {
                    assert_eq!(current, "private");
                    assert_eq!(*choice, 0);
                }
                _ => panic!("expected visibility"),
            },
            _ => panic!("edit did not open"),
        }
        app.on_key(key('j'));
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::Update {
                        patch, full_name, ..
                    },
                ] => {
                    assert_eq!(full_name, "splaice/garage");
                    assert_eq!(patch, &RepoPatch::Visibility("internal".to_string()));
                }
                _ => panic!("expected one update"),
            },
            _ => panic!("expected update"),
        }
        let generation = match &app.overlay {
            Some(Overlay::Edit(form)) => form.generation,
            _ => panic!("edit closed before the save finished"),
        };
        app.on_msg(Msg::Updated {
            generation,
            full_name: "splaice/garage".to_string(),
            result: Box::new(Ok(ApiRepo {
                full_name: "splaice/garage".to_string(),
                visibility: Some("internal".to_string()),
                ..ApiRepo::default()
            })),
        });
        assert!(app.overlay.is_none());
        assert_eq!(
            app.repos[0].fetched.as_ref().unwrap().visibility.as_deref(),
            Some("internal")
        );
        assert_eq!(
            app.repos[0].fetched.as_ref().unwrap().allow_merge_commit,
            Some(true)
        );
    }

    #[test]
    fn last_merge_style_is_not_submitted() {
        let mut app = fixture(vec![settings("private", true, false)]);
        app.focus = Focus::Detail;
        app.selected_field = FieldId::MergeCommit;
        match app.on_key(key('e')) {
            Effect::Jobs(_) => panic!("update was submitted"),
            Effect::None => {}
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        assert!(app.overlay.is_none());
        assert_eq!(
            app.toast.as_ref().map(|toast| toast.text.as_str()),
            Some("Keep merge commits or squash merges")
        );
    }

    #[test]
    fn y_copies_a_pull_request_and_reload_keeps_it() {
        let mut repo = sample("splaice", "garage");
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
        let mut app = fixture(vec![repo]);
        app.focus = Focus::Detail;
        app.on_key(key('j'));
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "https://example.com/9");
                assert_eq!(toast, "Copied pull request");
            }
            _ => panic!("expected copy"),
        }
        app.on_key(key('e'));
        assert!(app.overlay.is_none());
        app.repos[0].branches = vec![branch("main")];
        app.repos[0].commits = vec![commit("abcdef0123456789")];
        app.repos[0].commits_for = Some("main".to_string());
        app.on_msg(Msg::RepoList {
            generation: 0,
            result: Ok(vec![ListedRepo {
                owner: "splaice".to_string(),
                name: "garage".to_string(),
                clone_url: "https://origin.cursor.com/splaice/garage.git".to_string(),
            }]),
        });
        assert_eq!(app.repos[0].pulls.len(), 2);
        assert_eq!(app.repos[0].pulls[1].status, "closed");
        assert_eq!(app.repos[0].branches.len(), 1);
        assert_eq!(app.repos[0].commits.len(), 1);
        assert_eq!(app.repos[0].commits_for.as_deref(), Some("main"));
    }

    fn commit(sha: &str) -> Commit {
        Commit {
            sha: sha.to_string(),
            summary: "Add gate".to_string(),
            author: "Ada".to_string(),
            date: "2026-10-01T12:00:00Z".to_string(),
            url: String::new(),
        }
    }

    fn branch(name: &str) -> Branch {
        Branch {
            name: name.to_string(),
            sha: "a".repeat(40),
        }
    }

    #[test]
    fn j_onto_another_branch_requests_its_commits() {
        let mut repo = sample("acme", "widget");
        repo.branches = vec![branch("main"), branch("topic")];
        repo.commits = vec![commit("abcdef0123456789")];
        repo.commits_for = Some("main".to_string());
        let mut app = fixture(vec![repo]);
        app.focus = Focus::Detail;
        app.selected_branch = Some("main".to_string());
        match app.on_key(key('j')) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("the loaded branch was fetched again"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        assert!(matches!(app.cursor, DetailCursor::Branch(0)));
        match app.on_key(key('j')) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::Commits {
                        full_name, branch, ..
                    },
                ] => {
                    assert_eq!(full_name, "acme/widget");
                    assert_eq!(branch, "topic");
                }
                _ => panic!("expected one commits job"),
            },
            _ => panic!("expected commits job"),
        }
        assert!(matches!(app.cursor, DetailCursor::Branch(1)));
        assert_eq!(app.selected_branch.as_deref(), Some("topic"));
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "topic");
                assert_eq!(toast, "Copied branch");
            }
            _ => panic!("expected branch copy"),
        }
    }

    #[test]
    fn y_copies_the_full_commit_sha() {
        let mut repo = sample("acme", "widget");
        repo.branches = vec![branch("main")];
        repo.commits = vec![commit("abcdef0123456789")];
        repo.commits_for = Some("main".to_string());
        let mut app = fixture(vec![repo]);
        app.focus = Focus::Detail;
        app.selected_branch = Some("main".to_string());
        app.on_key(key('j'));
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "abcdef0123456789");
                assert_eq!(toast, "Copied commit");
            }
            _ => panic!("expected commit copy"),
        }
        app.on_key(key('e'));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn branches_select_the_default_until_a_row_is_chosen() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.detail_generation = 4;
        let jobs = app.on_msg(Msg::Branches {
            generation: 4,
            full_name: "acme/widget".to_string(),
            result: Ok(vec![branch("main"), branch("develop")]),
        });
        assert_eq!(app.repos[0].branches[0].name, "develop");
        match &jobs[..] {
            [Job::Commits { branch, .. }] => assert_eq!(branch, "develop"),
            _ => panic!("expected the first branch before a default is known"),
        }
        let jobs = app.on_msg(Msg::Detail {
            generation: 4,
            full_name: "acme/widget".to_string(),
            view: Box::new(Ok(ViewRepo {
                default_branch: Some("main".to_string()),
                ..ViewRepo::default()
            })),
            api: Box::new(Ok(ApiRepo {
                full_name: "acme/widget".to_string(),
                default_branch: Some("main".to_string()),
                ..ApiRepo::default()
            })),
        });
        assert_eq!(app.repos[0].branches[0].name, "main");
        assert_eq!(app.selected_branch.as_deref(), Some("main"));
        match &jobs[..] {
            [Job::Commits { branch, .. }] => assert_eq!(branch, "main"),
            _ => panic!("expected commits for the default branch"),
        }
        app.branch_locked = true;
        app.selected_branch = Some("develop".to_string());
        app.commits_inflight = false;
        app.cursor = DetailCursor::Commit(0);
        let jobs = app.on_msg(Msg::Detail {
            generation: 4,
            full_name: "acme/widget".to_string(),
            view: Box::new(Ok(ViewRepo {
                default_branch: Some("main".to_string()),
                ..ViewRepo::default()
            })),
            api: Box::new(Ok(ApiRepo {
                full_name: "acme/widget".to_string(),
                ..ApiRepo::default()
            })),
        });
        assert_eq!(app.selected_branch.as_deref(), Some("develop"));
        match &jobs[..] {
            [Job::Commits { branch, .. }] => assert_eq!(branch, "develop"),
            _ => panic!("a chosen branch should stay selected"),
        }
    }

    #[test]
    fn a_stale_commit_page_is_ignored() {
        let mut repo = sample("acme", "widget");
        repo.commits = vec![commit("newsha0123456789")];
        repo.commits_for = Some("main".to_string());
        let mut app = fixture(vec![repo]);
        app.selected_branch = Some("main".to_string());
        app.commits_generation = 2;
        app.on_msg(Msg::Commits {
            generation: 1,
            full_name: "acme/widget".to_string(),
            branch: "main".to_string(),
            result: Ok(vec![commit("oldsha0123456789")]),
        });
        assert_eq!(app.repos[0].commits[0].sha, "newsha0123456789");
    }
}
