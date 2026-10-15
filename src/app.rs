use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::buffer::Buffer;

use crate::github::{self, Child};
use crate::model::{
    ApiRepo, AuthStatus, ChildStatus, CreateForm, DeleteBranchForm, DeleteForm, DetailCursor,
    DetailSpan, DetailTarget, DisplayRow, EditForm, EditKind, Editor, FieldId, GITHUB_VISIBILITIES,
    GithubChildForm, ListedRepo, MirrorForm, Namespace, ReadmeView, Repo, RepoPatch, RowKind,
    SyncCommitsForm, VISIBILITIES, ViewRepo, detail_lines, display_rows, order_branches,
    preferred_branch, spans_from,
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
    Mirror(MirrorForm),
    GithubChild(GithubChildForm),
    SyncCommits(SyncCommitsForm),
    Delete(DeleteForm),
    DeleteBranch(DeleteBranchForm),
    Readme(ReadmeView),
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
    Readme {
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
    Mirror {
        generation: u64,
        github: String,
        namespace: String,
    },
    GithubSync {
        generation: u64,
        clone_url: String,
        github: String,
        visibility: String,
    },
    ChildStatus {
        generation: u64,
        full_name: String,
        github: String,
        branches: Vec<crate::model::Branch>,
    },
    SyncCommits {
        generation: u64,
        clone_url: String,
        github: String,
        visibility: String,
    },
    Delete {
        generation: u64,
        full_name: String,
    },
    DeleteBranch {
        generation: u64,
        full_name: String,
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
    Readme {
        generation: u64,
        full_name: String,
        branch: String,
        result: Result<origin::ReadmeFetch, String>,
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
    Mirrored {
        generation: u64,
        full_name: String,
        result: Result<(), String>,
    },
    GithubSynced {
        generation: u64,
        result: Result<(), String>,
    },
    ChildStatus {
        generation: u64,
        full_name: String,
        github: String,
        result: Result<github::Freshness, String>,
    },
    CommitsSynced {
        generation: u64,
        result: Result<(), String>,
    },
    Deleted {
        generation: u64,
        full_name: String,
        result: Result<(), String>,
    },
    BranchDeleted {
        generation: u64,
        full_name: String,
        branch: String,
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
    readme_inflight: bool,
    readme_branch: Option<String>,
    readme_rows: usize,
    readme_height: usize,
    child_check_inflight: bool,
    child_check_github: Option<String>,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    pub selection: Option<Selection>,
    pub screen: Screen,
    list_generation: u64,
    detail_generation: u64,
    create_generation: u64,
    mirror_generation: u64,
    github_generation: u64,
    delete_generation: u64,
    branch_delete_generation: u64,
    edit_generation: u64,
    commits_generation: u64,
    readme_generation: u64,
    child_check_generation: u64,
    pending_select: Option<String>,
    press: Option<Press>,
    children: BTreeMap<String, Child>,
    children_path: PathBuf,
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
            readme_inflight: false,
            readme_branch: None,
            readme_rows: 0,
            readme_height: 0,
            child_check_inflight: false,
            child_check_github: None,
            overlay: None,
            toast: None,
            selection: None,
            screen: Screen::default(),
            list_generation: 0,
            detail_generation: 0,
            create_generation: 0,
            mirror_generation: 0,
            github_generation: 0,
            delete_generation: 0,
            branch_delete_generation: 0,
            edit_generation: 0,
            commits_generation: 0,
            readme_generation: 0,
            child_check_generation: 0,
            pending_select: None,
            press: None,
            children: BTreeMap::new(),
            children_path: default_children_path(),
        }
    }
}

fn default_children_path() -> PathBuf {
    github::children_path().unwrap_or_else(|| std::env::temp_dir().join("origin-tui-children.json"))
}

impl App {
    pub fn boot() -> (Self, Vec<Job>) {
        let mut app = Self::default();
        match github::load_children(&app.children_path) {
            Ok(children) => app.children = children,
            Err(err) => app.toast(err, ToastKind::Err),
        }
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
        self.readme_rows = screen.readme_rows;
        self.readme_height = screen.readme_height;
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
        if query.is_empty() {
            return (0..self.repos.len()).collect();
        }
        // `splaice/name` contains "pl" and "plaice", so a full-name substring
        // keeps every repo in that namespace. The owner counts only as a prefix
        // that still leaves some other owner out.
        let owner_prefix_narrows = self
            .repos
            .iter()
            .any(|repo| !repo.owner.to_lowercase().starts_with(&query));
        self.repos
            .iter()
            .enumerate()
            .filter_map(|(index, repo)| {
                let name = repo.name.to_lowercase();
                let owner = repo.owner.to_lowercase();
                let full = repo.full_name.to_lowercase();
                let matches = name.contains(&query)
                    || (query.contains('/') && full.contains(&query))
                    || owner == query
                    || (owner_prefix_narrows && owner.starts_with(&query));
                matches.then_some(index)
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
        if let Some(Overlay::Mirror(form)) = &mut self.overlay {
            form.focused_editor().insert_str(&text);
            if truncated {
                self.toast("Paste truncated", ToastKind::Info);
            }
            return Effect::None;
        }
        if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
            if form.focus == 0 {
                form.github.insert_str(&text);
                if truncated {
                    self.toast("Paste truncated", ToastKind::Info);
                }
            }
            return Effect::None;
        }
        if let Some(Overlay::Delete(form)) = &mut self.overlay {
            form.confirm.insert_str(&text);
            if truncated {
                self.toast("Paste truncated", ToastKind::Info);
            }
            return Effect::None;
        }
        if let Some(Overlay::DeleteBranch(form)) = &mut self.overlay {
            form.confirm.insert_str(&text);
            if truncated {
                self.toast("Paste truncated", ToastKind::Info);
            }
            return Effect::None;
        }
        if matches!(
            self.overlay,
            Some(Overlay::Readme(_) | Overlay::SyncCommits(_))
        ) {
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
                                    repo.github_child = prev.github_child.clone();
                                    repo.child_status = prev.child_status.clone();
                                    repo.child_status_for = prev.child_status_for.clone();
                                    repo.readme = prev.readme.clone();
                                    repo.readme_error = prev.readme_error.clone();
                                    repo.readme_for = prev.readme_for.clone();
                                }
                                repo
                            })
                            .collect();
                        self.apply_children();
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
            Msg::Readme {
                generation,
                full_name,
                branch,
                result,
            } => {
                if generation != self.readme_generation {
                    return Vec::new();
                }
                self.readme_inflight = false;
                if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                {
                    repo.readme_for = Some(branch);
                    match result {
                        Ok(origin::ReadmeFetch::Missing) => {
                            repo.readme = None;
                            repo.readme_error = None;
                        }
                        Ok(origin::ReadmeFetch::Found(readme)) => {
                            repo.readme = Some(readme);
                            repo.readme_error = None;
                        }
                        Err(err) => {
                            repo.readme = None;
                            repo.readme_error = Some(err);
                        }
                    }
                }
                if self.selected_field == FieldId::Readme
                    && !self.field_ids().contains(&FieldId::Readme)
                {
                    let ids = self.field_ids();
                    self.selected_field = if ids.contains(&FieldId::Clone) {
                        FieldId::Clone
                    } else {
                        ids.first().copied().unwrap_or(FieldId::Name)
                    };
                }
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
            Msg::Mirrored {
                generation,
                full_name,
                result,
            } => {
                if generation != self.mirror_generation {
                    return Vec::new();
                }
                match result {
                    Ok(()) => {
                        self.overlay = None;
                        self.pending_select = Some(full_name);
                        self.focus = Focus::List;
                        self.toast("GitHub mirror created", ToastKind::Info);
                        vec![self.request_list()]
                    }
                    Err(err) => {
                        if let Some(Overlay::Mirror(form)) = &mut self.overlay {
                            form.busy = false;
                            form.error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
            Msg::GithubSynced { generation, result } => {
                let current = generation == self.github_generation;
                match result {
                    Ok(()) => {
                        if current && matches!(self.overlay, Some(Overlay::GithubChild(_))) {
                            self.overlay = None;
                        }
                        self.toast("Synced to GitHub", ToastKind::Info);
                        if current {
                            self.ensure_child_status(true)
                        } else {
                            Vec::new()
                        }
                    }
                    Err(err) => {
                        if current && let Some(Overlay::GithubChild(form)) = &mut self.overlay {
                            form.busy = false;
                            form.error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
            Msg::ChildStatus {
                generation,
                full_name,
                github,
                result,
            } => {
                if generation != self.child_check_generation {
                    return Vec::new();
                }
                self.child_check_inflight = false;
                if let Some(repo) = self
                    .repos
                    .iter_mut()
                    .find(|repo| repo.full_name == full_name)
                    && repo.github_child.as_deref() == Some(github.as_str())
                {
                    repo.child_status_for = Some(github);
                    repo.child_status = Some(match result {
                        Ok(github::Freshness::Current) => ChildStatus::Current,
                        Ok(github::Freshness::Stale(names)) => ChildStatus::Stale(names),
                        Err(err) if github::remote_missing(&err) => ChildStatus::Missing,
                        Err(err) => ChildStatus::Failed(err),
                    });
                }
                Vec::new()
            }
            Msg::CommitsSynced { generation, result } => {
                let current = generation == self.github_generation;
                let jobs = match result {
                    Ok(()) => {
                        if current && matches!(self.overlay, Some(Overlay::SyncCommits(_))) {
                            self.overlay = None;
                        }
                        self.toast("Synced commits to GitHub", ToastKind::Info);
                        Vec::new()
                    }
                    Err(err) => {
                        if current && let Some(Overlay::SyncCommits(form)) = &mut self.overlay {
                            form.busy = false;
                            form.error = Some(err);
                        } else if current {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                };
                if current {
                    let mut jobs = jobs;
                    jobs.extend(self.ensure_child_status(true));
                    jobs
                } else {
                    jobs
                }
            }
            Msg::Deleted {
                generation,
                full_name,
                result,
            } => {
                if generation != self.delete_generation {
                    return Vec::new();
                }
                match result {
                    Ok(()) => {
                        self.overlay = None;
                        match self.forget_child(&full_name) {
                            Ok(()) => self.toast("Repository deleted", ToastKind::Info),
                            Err(err) => {
                                self.toast(format!("Repository deleted. {err}"), ToastKind::Err);
                            }
                        }
                        vec![self.request_list()]
                    }
                    Err(err) => {
                        if let Some(Overlay::Delete(form)) = &mut self.overlay {
                            form.busy = false;
                            form.error = Some(err);
                        } else {
                            self.toast(err, ToastKind::Err);
                        }
                        Vec::new()
                    }
                }
            }
            Msg::BranchDeleted {
                generation,
                full_name,
                branch,
                result,
            } => {
                if generation != self.branch_delete_generation {
                    return Vec::new();
                }
                match result {
                    Ok(()) => self.after_branch_deleted(&full_name, &branch),
                    Err(err) => {
                        if let Some(Overlay::DeleteBranch(form)) = &mut self.overlay {
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

    fn after_branch_deleted(&mut self, full_name: &str, branch: &str) -> Vec<Job> {
        self.overlay = None;
        self.toast("Branch deleted", ToastKind::Info);
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|repo| repo.full_name == full_name)
        {
            repo.branches.retain(|item| item.name != branch);
            repo.branches_error = None;
        }
        let selected = self.selected_name().as_deref() == Some(full_name);
        let viewing = selected && self.selected_branch.as_deref() == Some(branch);
        if viewing {
            self.selected_branch = None;
            self.branch_locked = false;
        }
        let mut jobs = Vec::new();
        if viewing {
            jobs.extend(self.sync_branch(true));
        }
        if selected && matches!(self.cursor, DetailCursor::Branch(_)) {
            self.place_branch_cursor();
        }
        if !selected {
            return jobs;
        }
        self.detail_generation += 1;
        self.branches_inflight = true;
        self.pulls_inflight = true;
        let generation = self.detail_generation;
        let full_name = full_name.to_string();
        if self.detail_inflight {
            jobs.push(Job::Detail {
                full_name: full_name.clone(),
                generation,
            });
        }
        jobs.push(Job::Branches {
            full_name: full_name.clone(),
            generation,
        });
        jobs.push(Job::Pulls {
            full_name,
            generation,
        });
        jobs
    }

    fn forget_child(&mut self, full_name: &str) -> Result<(), String> {
        if self.children.remove(full_name).is_none() {
            return Ok(());
        }
        github::save_children(&self.children_path, &self.children)
    }

    fn apply_children(&mut self) {
        for repo in &mut self.repos {
            if let Some(child) = self.children.get(&repo.full_name) {
                if repo.child_status_for.as_deref() != Some(child.github.as_str()) {
                    repo.child_status = None;
                    repo.child_status_for = None;
                }
                repo.github_child = Some(child.github.clone());
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
                    Some(Overlay::Mirror(form)) => form.focus = index,
                    Some(Overlay::GithubChild(form)) => {
                        if index == 0 {
                            form.focus = 0;
                        } else {
                            let choice = index - 1;
                            if choice < GITHUB_VISIBILITIES.len() {
                                form.focus = 1;
                                form.visibility = choice;
                            }
                        }
                    }
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
        let mirror = self.screen.hits.mirror;
        let sync = self.screen.hits.sync;
        let delete = self.screen.hits.delete;
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
        if mirror.is_some_and(|rect| contains(rect, x, y)) {
            self.open_mirror();
            return Effect::None;
        }
        if sync.is_some_and(|rect| contains(rect, x, y)) {
            self.open_github_child();
            return Effect::None;
        }
        if delete.is_some_and(|rect| contains(rect, x, y)) {
            self.open_delete();
            return Effect::None;
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
            if id == FieldId::Readme {
                return self.open_readme();
            }
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
        if matches!(self.overlay, Some(Overlay::Readme(_))) {
            self.scroll_readme(delta);
            return Effect::None;
        }
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
        if matches!(self.overlay, Some(Overlay::Mirror(_))) {
            return self.mirror_overlay_key(key);
        }
        if matches!(self.overlay, Some(Overlay::GithubChild(_))) {
            return self.github_overlay_key(key);
        }
        if matches!(self.overlay, Some(Overlay::SyncCommits(_))) {
            return self.sync_commits_overlay_key(key);
        }
        if matches!(self.overlay, Some(Overlay::Delete(_))) {
            return self.delete_overlay_key(key);
        }
        if matches!(self.overlay, Some(Overlay::DeleteBranch(_))) {
            return self.delete_branch_overlay_key(key);
        }
        if matches!(self.overlay, Some(Overlay::Readme(_))) {
            return self.readme_overlay_key(key);
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
            KeyCode::Char('y') => self.copy_focused(),
            KeyCode::Enter => self.activate_focused(),
            KeyCode::Char('e')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.begin_edit()
            }
            KeyCode::Char('x')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.open_delete_branch()
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
            KeyCode::Char('m') => {
                self.open_mirror();
                Some(Effect::None)
            }
            KeyCode::Char('s') => {
                self.open_github_child();
                Some(Effect::None)
            }
            KeyCode::Char('u') => {
                self.open_sync_commits();
                Some(Effect::None)
            }
            KeyCode::Char('d') => {
                self.open_delete();
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
            self.readme_inflight = false;
            self.child_check_inflight = false;
            return Vec::new();
        }
        if previous != next {
            self.detail_scroll = 0;
            self.selected_field = FieldId::Clone;
            self.cursor = DetailCursor::Field;
            self.selected_branch = None;
            self.branch_locked = false;
            self.readme_generation += 1;
            self.readme_inflight = false;
            self.readme_branch = None;
            self.child_check_generation += 1;
            self.child_check_inflight = false;
            self.child_check_github = None;
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
        let mut jobs = if let Some(name) =
            locked.or_else(|| self.selected_repo().and_then(preferred_branch))
        {
            self.select_branch(name, force)
        } else {
            let branch = self
                .selected_repo()
                .and_then(|repo| repo.default_branch_name())
                .unwrap_or_default();
            self.ensure_readme(&branch, force)
        };
        jobs.extend(self.ensure_child_status(force));
        jobs
    }

    fn ensure_child_status(&mut self, force: bool) -> Vec<Job> {
        if self.branches_inflight {
            return Vec::new();
        }
        let Some(full_name) = self.selected_name() else {
            return Vec::new();
        };
        let prepared = self.selected_repo().and_then(|repo| {
            if repo.mirrored() {
                return None;
            }
            let github = repo.github_child.clone()?;
            if repo.branches_error.is_some() && repo.branches.is_empty() {
                return None;
            }
            let settled = repo.child_status_for.as_deref() == Some(github.as_str())
                && repo
                    .child_status
                    .as_ref()
                    .is_some_and(|status| !matches!(status, ChildStatus::Checking));
            Some((github, repo.branches.clone(), settled))
        });
        let Some((github, branches, settled)) = prepared else {
            return Vec::new();
        };
        let same = self.child_check_github.as_deref() == Some(github.as_str());
        if self.child_check_inflight && same && !force {
            return Vec::new();
        }
        if settled && !force {
            return Vec::new();
        }
        self.child_check_generation += 1;
        let generation = self.child_check_generation;
        self.child_check_inflight = true;
        self.child_check_github = Some(github.clone());
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|repo| repo.full_name == full_name)
            && repo.child_status_for.as_deref() != Some(github.as_str())
        {
            repo.child_status = Some(ChildStatus::Checking);
            repo.child_status_for = Some(github.clone());
        }
        vec![Job::ChildStatus {
            generation,
            full_name,
            github,
            branches,
        }]
    }

    fn select_branch(&mut self, name: String, force: bool) -> Vec<Job> {
        let already = self.selected_branch.as_deref() == Some(name.as_str());
        self.selected_branch = Some(name.clone());
        let loaded = self.selected_repo().is_some_and(|repo| {
            repo.commits_for.as_deref() == Some(name.as_str()) && repo.commits_error.is_none()
        });
        let skip_commits =
            !force && (loaded && !self.commits_inflight || already && self.commits_inflight);
        let mut jobs = Vec::new();
        if !skip_commits && let Some(full_name) = self.selected_name() {
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
            jobs.push(Job::Commits {
                full_name,
                branch: name.clone(),
                generation,
            });
        }
        jobs.extend(self.ensure_readme(&name, force));
        jobs
    }

    fn ensure_readme(&mut self, branch: &str, force: bool) -> Vec<Job> {
        let Some(full_name) = self.selected_name() else {
            return Vec::new();
        };
        if self.readme_inflight && self.readme_branch.as_deref() == Some(branch) {
            return Vec::new();
        }
        let current = self.selected_repo().is_some_and(|repo| {
            repo.readme_for.as_deref() == Some(branch) && repo.readme_error.is_none()
        });
        if current && !force {
            return Vec::new();
        }
        self.readme_generation += 1;
        let generation = self.readme_generation;
        self.readme_inflight = true;
        self.readme_branch = Some(branch.to_string());
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|repo| repo.full_name == full_name)
            && repo.readme_for.as_deref() != Some(branch)
        {
            repo.readme = None;
            repo.readme_error = None;
            repo.readme_for = None;
        }
        vec![Job::Readme {
            full_name,
            branch: branch.to_string(),
            generation,
        }]
    }

    fn open_readme(&mut self) -> Effect {
        let prepared = self.selected_repo().and_then(|repo| {
            let readme = repo.readme.clone()?;
            Some((repo.full_name.clone(), readme.path, readme.text))
        });
        let Some((full_name, path, text)) = prepared else {
            self.toast("This repository has no README.md", ToastKind::Info);
            return Effect::None;
        };
        self.overlay = Some(Overlay::Readme(ReadmeView {
            full_name,
            path,
            text,
            scroll: 0,
        }));
        Effect::None
    }

    fn activate_focused(&mut self) -> Effect {
        if matches!(self.cursor, DetailCursor::Field)
            && self.selected_field == FieldId::Readme
            && self
                .selected_repo()
                .is_some_and(|repo| repo.readme.is_some())
        {
            return self.open_readme();
        }
        self.copy_focused()
    }

    fn readme_overlay_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Char('q') => Effect::Quit,
            KeyCode::Char('y') => self.copy_readme(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_readme(-1);
                Effect::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_readme(1);
                Effect::None
            }
            KeyCode::PageUp | KeyCode::Char('u')
                if key.code == KeyCode::PageUp || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.scroll_readme(-(self.readme_height.max(1) as isize));
                Effect::None
            }
            KeyCode::PageDown | KeyCode::Char('d')
                if key.code == KeyCode::PageDown
                    || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.scroll_readme(self.readme_height.max(1) as isize);
                Effect::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_readme(-10_000);
                Effect::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_readme(10_000);
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn copy_readme(&mut self) -> Effect {
        let Some(Overlay::Readme(view)) = &self.overlay else {
            return Effect::None;
        };
        if view.text.is_empty() {
            self.toast("Nothing to copy", ToastKind::Err);
            return Effect::None;
        }
        Effect::Copy {
            text: view.text.clone(),
            toast: "Copied README".to_string(),
        }
    }

    fn scroll_readme(&mut self, delta: isize) {
        let max = self.readme_rows.saturating_sub(self.readme_height.max(1));
        let Some(Overlay::Readme(view)) = &mut self.overlay else {
            return;
        };
        let next = view.scroll as isize + delta;
        view.scroll = next.clamp(0, max as isize) as usize;
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

    fn open_mirror(&mut self) {
        self.mirror_generation += 1;
        let namespace = self.default_namespace();
        self.overlay = Some(Overlay::Mirror(MirrorForm {
            github: Editor::new(""),
            namespace: Editor::new(namespace),
            focus: 0,
            error: None,
            busy: false,
            generation: self.mirror_generation,
        }));
    }

    fn mirror_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::Mirror(ref form)) if form.busy);
        if busy {
            self.toast("Still creating the mirror", ToastKind::Info);
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_mirror(),
            KeyCode::Tab | KeyCode::Down => {
                self.cycle_mirror(1);
                Effect::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.cycle_mirror(-1);
                Effect::None
            }
            _ => {
                if let Some(Overlay::Mirror(form)) = &mut self.overlay {
                    edit_key(form.focused_editor(), &key);
                }
                Effect::None
            }
        }
    }

    fn cycle_mirror(&mut self, delta: isize) {
        if let Some(Overlay::Mirror(form)) = &mut self.overlay {
            form.focus = (form.focus as isize + delta).rem_euclid(2) as usize;
        }
    }

    fn submit_mirror(&mut self) -> Effect {
        let Some(Overlay::Mirror(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let github = form.github.value.trim().to_string();
        let namespace = form.namespace.value.trim().to_string();
        let generation = form.generation;
        if let Err(message) = validate_mirror(&github, &namespace) {
            if let Some(Overlay::Mirror(form)) = &mut self.overlay {
                form.error = Some(message);
            }
            return Effect::None;
        }
        if let Some(Overlay::Mirror(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::Mirror {
            generation,
            github,
            namespace,
        }])
    }

    fn open_github_child(&mut self) {
        let waiting = self.detail_inflight;
        let prepared = self.selected_repo().map(|repo| {
            (
                repo.view.is_none() && repo.api().is_none(),
                repo.mirrored(),
                repo.full_name.clone(),
                repo.github_child.clone(),
            )
        });
        let Some((unknown, mirrored, origin_name, existing)) = prepared else {
            self.toast("Select a repository", ToastKind::Info);
            return;
        };
        if unknown && waiting {
            self.toast("Repository details are still loading", ToastKind::Info);
            return;
        }
        if mirrored {
            self.toast(
                "GitHub is already the source. Origin is not.",
                ToastKind::Info,
            );
            return;
        }
        let saved = self.children.get(&origin_name).cloned();
        let github = saved
            .as_ref()
            .map(|child| child.github.clone())
            .or(existing)
            .unwrap_or(origin_name);
        let visibility = saved
            .as_ref()
            .and_then(|child| {
                GITHUB_VISIBILITIES
                    .iter()
                    .position(|item| *item == child.visibility)
            })
            .unwrap_or(0);
        self.github_generation += 1;
        self.overlay = Some(Overlay::GithubChild(GithubChildForm {
            github: Editor::new(github),
            visibility,
            focus: 0,
            error: None,
            busy: false,
            generation: self.github_generation,
        }));
    }

    fn github_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::GithubChild(ref form)) if form.busy);
        if busy {
            self.toast("Still syncing to GitHub", ToastKind::Info);
            return Effect::None;
        }
        let visibility =
            matches!(self.overlay, Some(Overlay::GithubChild(ref form)) if form.focus == 1);
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_github_child(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.cycle_github_focus();
                Effect::None
            }
            KeyCode::Up | KeyCode::Char('k') if visibility => {
                self.cycle_github_visibility(-1);
                Effect::None
            }
            KeyCode::Down | KeyCode::Char('j') if visibility => {
                self.cycle_github_visibility(1);
                Effect::None
            }
            KeyCode::Left if visibility => {
                self.cycle_github_visibility(-1);
                Effect::None
            }
            KeyCode::Right if visibility => {
                self.cycle_github_visibility(1);
                Effect::None
            }
            KeyCode::Down | KeyCode::Up => {
                self.cycle_github_focus();
                Effect::None
            }
            _ => {
                if let Some(Overlay::GithubChild(form)) = &mut self.overlay
                    && form.focus == 0
                {
                    edit_key(&mut form.github, &key);
                }
                Effect::None
            }
        }
    }

    fn cycle_github_focus(&mut self) {
        if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
            form.focus = if form.focus == 0 { 1 } else { 0 };
        }
    }

    fn cycle_github_visibility(&mut self, delta: isize) {
        if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
            let len = GITHUB_VISIBILITIES.len() as isize;
            form.visibility = (form.visibility as isize + delta).rem_euclid(len) as usize;
        }
    }

    fn submit_github_child(&mut self) -> Effect {
        let Some(Overlay::GithubChild(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let github = form.github.value.trim().to_string();
        let visibility = GITHUB_VISIBILITIES
            .get(form.visibility)
            .copied()
            .unwrap_or("private")
            .to_string();
        let generation = form.generation;
        if let Err(message) = validate_github_repo(&github) {
            if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
                form.error = Some(message);
            }
            return Effect::None;
        }
        let Some(origin_name) = self.selected_name() else {
            return Effect::None;
        };
        let Some(clone_url) = self
            .selected_repo()
            .and_then(|repo| repo.clone_url().map(str::to_string))
        else {
            if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
                form.error = Some("This repository has no clone URL.".to_string());
            }
            return Effect::None;
        };
        let mut next = self.children.clone();
        next.insert(
            origin_name.clone(),
            Child {
                github: github.clone(),
                visibility: visibility.clone(),
            },
        );
        if let Err(err) = github::save_children(&self.children_path, &next) {
            if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
                form.error = Some(err);
            }
            return Effect::None;
        }
        self.children = next;
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|repo| repo.full_name == origin_name)
        {
            repo.github_child = Some(github.clone());
        }
        if let Some(Overlay::GithubChild(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::GithubSync {
            generation,
            clone_url,
            github,
            visibility,
        }])
    }

    fn open_sync_commits(&mut self) {
        let waiting = self.detail_inflight;
        let prepared = self.selected_repo().map(|repo| {
            (
                repo.view.is_none() && repo.api().is_none(),
                repo.mirrored(),
                repo.full_name.clone(),
                repo.github_child.clone(),
                repo.clone_url().map(str::to_string),
            )
        });
        let Some((unknown, mirrored, origin_name, github, clone_url)) = prepared else {
            self.toast("Select a repository", ToastKind::Info);
            return;
        };
        let Some(github) = github else {
            self.toast("This repository has no GitHub child.", ToastKind::Info);
            return;
        };
        if unknown && waiting {
            self.toast("Repository details are still loading", ToastKind::Info);
            return;
        }
        if mirrored {
            self.toast(
                "GitHub is already the source. Origin is not.",
                ToastKind::Info,
            );
            return;
        }
        let Some(clone_url) = clone_url else {
            self.toast("This repository has no clone URL.", ToastKind::Info);
            return;
        };
        let visibility = self
            .children
            .get(&origin_name)
            .map(|child| child.visibility.clone())
            .unwrap_or_else(|| "private".to_string());
        self.github_generation += 1;
        self.overlay = Some(Overlay::SyncCommits(SyncCommitsForm {
            full_name: origin_name,
            github,
            clone_url,
            visibility,
            error: None,
            busy: false,
            generation: self.github_generation,
        }));
    }

    fn sync_commits_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::SyncCommits(ref form)) if form.busy);
        if busy {
            self.toast("Still syncing commits", ToastKind::Info);
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_sync_commits(),
            _ => Effect::None,
        }
    }

    fn submit_sync_commits(&mut self) -> Effect {
        let Some(Overlay::SyncCommits(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let generation = form.generation;
        let clone_url = form.clone_url.clone();
        let github = form.github.clone();
        let visibility = form.visibility.clone();
        if let Some(Overlay::SyncCommits(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::SyncCommits {
            generation,
            clone_url,
            github,
            visibility,
        }])
    }

    fn open_delete(&mut self) {
        let prepared = self
            .selected_repo()
            .map(|repo| (repo.full_name.clone(), repo.mirrored()));
        let Some((full_name, mirrored)) = prepared else {
            self.toast("Select a repository", ToastKind::Info);
            return;
        };
        self.delete_generation += 1;
        self.overlay = Some(Overlay::Delete(DeleteForm {
            full_name,
            confirm: Editor::new(""),
            mirrored,
            error: None,
            busy: false,
            generation: self.delete_generation,
        }));
    }

    fn delete_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::Delete(ref form)) if form.busy);
        if busy {
            self.toast("Still deleting the repository", ToastKind::Info);
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_delete(),
            _ => {
                if let Some(Overlay::Delete(form)) = &mut self.overlay {
                    edit_key(&mut form.confirm, &key);
                }
                Effect::None
            }
        }
    }

    fn submit_delete(&mut self) -> Effect {
        let Some(Overlay::Delete(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let full_name = form.full_name.clone();
        let typed = form.confirm.value.trim().to_string();
        let generation = form.generation;
        if typed != full_name {
            if let Some(Overlay::Delete(form)) = &mut self.overlay {
                form.error = Some(format!("Type {full_name} to confirm."));
            }
            return Effect::None;
        }
        if let Some(Overlay::Delete(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::Delete {
            generation,
            full_name,
        }])
    }

    fn open_delete_branch(&mut self) -> Effect {
        let DetailCursor::Branch(index) = self.cursor else {
            self.toast("Select a branch", ToastKind::Info);
            return Effect::None;
        };
        let waiting = self.detail_inflight;
        let prepared = self.selected_repo().map(|repo| {
            (
                repo.view.is_none() && repo.api().is_none(),
                repo.mirrored(),
                repo.full_name.clone(),
                repo.default_branch_name(),
                repo.branches.get(index).map(|branch| branch.name.clone()),
            )
        });
        let Some((unknown, mirrored, full_name, default_name, branch)) = prepared else {
            self.toast("Select a repository", ToastKind::Info);
            return Effect::None;
        };
        let Some(branch) = branch else {
            self.toast("Select a branch", ToastKind::Info);
            return Effect::None;
        };
        if unknown && waiting {
            self.toast("Repository details are still loading", ToastKind::Info);
            return Effect::None;
        }
        if mirrored {
            self.toast(
                "GitHub is the source. Delete the branch there.",
                ToastKind::Info,
            );
            return Effect::None;
        }
        if default_name.as_deref() == Some(branch.as_str()) {
            self.toast("The default branch can't be deleted.", ToastKind::Info);
            return Effect::None;
        }
        if origin::delete_branch_args(&full_name, &branch).is_err() {
            self.toast("That branch name can't be deleted.", ToastKind::Info);
            return Effect::None;
        }
        self.branch_delete_generation += 1;
        self.overlay = Some(Overlay::DeleteBranch(DeleteBranchForm {
            full_name,
            branch,
            confirm: Editor::new(""),
            error: None,
            busy: false,
            generation: self.branch_delete_generation,
        }));
        Effect::None
    }

    fn delete_branch_overlay_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(self.overlay, Some(Overlay::DeleteBranch(ref form)) if form.busy);
        if busy {
            self.toast("Still deleting the branch", ToastKind::Info);
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_delete_branch(),
            _ => {
                if let Some(Overlay::DeleteBranch(form)) = &mut self.overlay {
                    edit_key(&mut form.confirm, &key);
                }
                Effect::None
            }
        }
    }

    fn submit_delete_branch(&mut self) -> Effect {
        let Some(Overlay::DeleteBranch(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let full_name = form.full_name.clone();
        let branch = form.branch.clone();
        let typed = form.confirm.value.trim().to_string();
        let generation = form.generation;
        if typed != branch {
            if let Some(Overlay::DeleteBranch(form)) = &mut self.overlay {
                form.error = Some(format!("Type {branch} to confirm."));
            }
            return Effect::None;
        }
        if let Some(Overlay::DeleteBranch(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        Effect::Jobs(vec![Job::DeleteBranch {
            generation,
            full_name,
            branch,
        }])
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

fn validate_github_repo(github: &str) -> Result<(), String> {
    let Some((owner, name)) = github.split_once('/') else {
        return Err("Use a GitHub repository as owner/repo.".to_string());
    };
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || owner.starts_with('-')
        || name.starts_with('-')
        || !valid_token(owner, false)
        || !valid_token(name, false)
    {
        return Err("Use a GitHub repository as owner/repo.".to_string());
    }
    Ok(())
}

fn validate_mirror(github: &str, namespace: &str) -> Result<(), String> {
    validate_github_repo(github)?;
    if !namespace.is_empty() && !valid_token(namespace, false) {
        return Err("Namespace uses letters, numbers, dots, underscores, and hyphens.".to_string());
    }
    Ok(())
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
    fn filter_does_not_match_the_owner_buried_in_the_full_name() {
        let mut app = fixture(vec![
            sample("splaice", "origin-tui"),
            sample("splaice", "garage"),
            sample("acme", "planner"),
        ]);
        app.on_key(key('/'));
        for ch in "pla".chars() {
            app.on_key(key(ch));
        }
        assert_eq!(repo_labels(&app), vec!["planner".to_string()]);
        for ch in "ice".chars() {
            app.on_key(key(ch));
        }
        assert!(repo_labels(&app).is_empty());
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.on_key(key('/'));
        for ch in "spl".chars() {
            app.on_key(key(ch));
        }
        assert_eq!(
            repo_labels(&app),
            vec!["origin-tui".to_string(), "garage".to_string()]
        );
        for ch in "aice/gar".chars() {
            app.on_key(key(ch));
        }
        assert_eq!(repo_labels(&app), vec!["garage".to_string()]);
    }

    fn repo_labels(app: &App) -> Vec<String> {
        app.rows()
            .iter()
            .filter(|row| matches!(row.kind, RowKind::Repo(_)))
            .map(|row| row.label.clone())
            .collect()
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
    fn m_requests_a_mirror_of_a_github_repo() {
        let mut app = fixture(vec![sample("splaice", "dotfiles")]);
        app.on_key(key('m'));
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("an empty GitHub repo was submitted"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::Mirror(form)) => assert!(form.error.is_some()),
            _ => panic!("form closed"),
        }
        for ch in "acme/checkout".chars() {
            app.on_key(key(ch));
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::Mirror {
                        github, namespace, ..
                    },
                ] => {
                    assert_eq!(github, "acme/checkout");
                    assert_eq!(namespace, "splaice");
                }
                _ => panic!("expected a mirror job"),
            },
            _ => panic!("expected a mirror job"),
        }
    }

    #[test]
    fn s_requests_a_sync_to_the_github_child() {
        let path =
            std::env::temp_dir().join(format!("ot-children-{}-sync.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut app = fixture(vec![sample("splaice", "dotfiles")]);
        app.children_path = path.clone();
        app.on_key(key('s'));
        match &app.overlay {
            Some(Overlay::GithubChild(form)) => {
                assert_eq!(form.github.value, "splaice/dotfiles");
            }
            _ => panic!("sync form did not open"),
        }
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("an empty GitHub repo was submitted"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::GithubChild(form)) => assert!(form.error.is_some()),
            _ => panic!("form closed"),
        }
        for ch in "acme/widget".chars() {
            app.on_key(key(ch));
        }
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.on_key(key('j'));
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::GithubSync {
                        github,
                        visibility,
                        clone_url,
                        ..
                    },
                ] => {
                    assert_eq!(github, "acme/widget");
                    assert_eq!(visibility, "public");
                    assert_eq!(clone_url, "https://origin.cursor.com/splaice/dotfiles.git");
                }
                _ => panic!("expected a sync job"),
            },
            _ => panic!("expected a sync job"),
        }
        assert_eq!(app.repos[0].github_child.as_deref(), Some("acme/widget"));
        let saved = github::load_children(&path).unwrap();
        assert_eq!(saved["splaice/dotfiles"].github, "acme/widget");
        assert_eq!(saved["splaice/dotfiles"].visibility, "public");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn s_refuses_an_inbound_github_mirror() {
        let mut repo = sample("acme", "widget");
        repo.fetched = Some(ApiRepo {
            full_name: "acme/widget".to_string(),
            mirror_source: Some("github".to_string()),
            mirror_status: Some("inbound".to_string()),
            ..ApiRepo::default()
        });
        let mut app = fixture(vec![repo]);
        match app.on_key(key('s')) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("a mirror was synced"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(
            toast.text.contains("GitHub is already the source"),
            "{}",
            toast.text
        );
    }

    #[test]
    fn s_waits_until_mirror_status_is_known() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.detail_inflight = true;
        app.on_key(key('s'));
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.text.contains("still loading"), "{}", toast.text);
    }

    #[test]
    fn saved_child_prefills_the_sync_form() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.children.insert(
            "acme/widget".to_string(),
            Child {
                github: "acme/checkout".to_string(),
                visibility: "internal".to_string(),
            },
        );
        app.on_key(key('s'));
        match &app.overlay {
            Some(Overlay::GithubChild(form)) => {
                assert_eq!(form.github.value, "acme/checkout");
                assert_eq!(GITHUB_VISIBILITIES[form.visibility], "internal");
            }
            _ => panic!("sync form did not open"),
        }
    }

    #[test]
    fn list_refresh_keeps_a_github_child() {
        let mut kept = sample("acme", "widget");
        kept.github_child = Some("acme/checkout".to_string());
        kept.readme = Some(crate::model::Readme {
            path: "README.md".to_string(),
            text: "# Hi".to_string(),
        });
        kept.readme_for = Some("main".to_string());
        let mut app = fixture(vec![kept, sample("acme", "other")]);
        app.children.insert(
            "acme/other".to_string(),
            Child {
                github: "acme/widget".to_string(),
                visibility: "private".to_string(),
            },
        );
        app.on_msg(Msg::RepoList {
            generation: 0,
            result: Ok(vec![
                ListedRepo {
                    owner: "acme".to_string(),
                    name: "widget".to_string(),
                    clone_url: "https://origin.cursor.com/acme/widget.git".to_string(),
                },
                ListedRepo {
                    owner: "acme".to_string(),
                    name: "other".to_string(),
                    clone_url: "https://origin.cursor.com/acme/other.git".to_string(),
                },
            ]),
        });
        assert_eq!(app.repos[0].github_child.as_deref(), Some("acme/checkout"));
        assert_eq!(app.repos[1].github_child.as_deref(), Some("acme/widget"));
        assert_eq!(
            app.repos[0]
                .readme
                .as_ref()
                .map(|readme| readme.text.as_str()),
            Some("# Hi")
        );
    }

    #[test]
    fn branches_check_whether_the_github_child_is_current() {
        let mut repo = sample("acme", "widget");
        repo.github_child = Some("acme/checkout".to_string());
        let mut app = fixture(vec![repo]);
        app.branches_inflight = true;
        app.detail_generation = 3;
        let jobs = app.on_msg(Msg::Branches {
            generation: 3,
            full_name: "acme/widget".to_string(),
            result: Ok(vec![
                Branch {
                    name: "main".to_string(),
                    sha: "abc".to_string(),
                },
                Branch {
                    name: "topic".to_string(),
                    sha: "def".to_string(),
                },
            ]),
        });
        assert!(jobs.iter().any(|job| matches!(
            job,
            Job::ChildStatus { github, full_name, branches, .. }
                if github == "acme/checkout"
                    && full_name == "acme/widget"
                    && branches.iter().any(|branch| branch.name == "main" && branch.sha == "abc")
        )));
        assert_eq!(app.repos[0].child_status, Some(ChildStatus::Checking));
    }

    #[test]
    fn a_repo_without_a_child_skips_the_status_check() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.branches_inflight = true;
        let jobs = app.on_msg(Msg::Branches {
            generation: 0,
            full_name: "acme/widget".to_string(),
            result: Ok(vec![Branch {
                name: "main".to_string(),
                sha: "abc".to_string(),
            }]),
        });
        assert!(
            jobs.iter()
                .all(|job| !matches!(job, Job::ChildStatus { .. }))
        );
    }

    #[test]
    fn child_status_is_shown_and_a_stale_result_is_ignored() {
        let mut repo = sample("acme", "widget");
        repo.github_child = Some("acme/checkout".to_string());
        let mut app = fixture(vec![repo]);
        app.child_check_generation = 4;
        app.on_msg(Msg::ChildStatus {
            generation: 4,
            full_name: "acme/widget".to_string(),
            github: "acme/checkout".to_string(),
            result: Ok(github::Freshness::Stale(vec!["main".to_string()])),
        });
        let field = app.repos[0]
            .fields()
            .into_iter()
            .find(|field| field.id == FieldId::ChildStatus)
            .unwrap();
        assert_eq!(field.display, "not up to date · main");
        app.on_msg(Msg::ChildStatus {
            generation: 2,
            full_name: "acme/widget".to_string(),
            github: "acme/checkout".to_string(),
            result: Ok(github::Freshness::Current),
        });
        assert_eq!(
            app.repos[0].child_status,
            Some(ChildStatus::Stale(vec!["main".to_string()]))
        );
        app.on_msg(Msg::ChildStatus {
            generation: 4,
            full_name: "acme/widget".to_string(),
            github: "acme/checkout".to_string(),
            result: Err("git: ERROR: Repository not found.".to_string()),
        });
        assert_eq!(app.repos[0].child_status, Some(ChildStatus::Missing));
    }

    #[test]
    fn u_pushes_origin_commits_to_the_github_child() {
        let mut repo = sample("acme", "widget");
        repo.github_child = Some("acme/checkout".to_string());
        let mut app = fixture(vec![repo]);
        app.on_key(key('u'));
        match &app.overlay {
            Some(Overlay::SyncCommits(form)) => {
                assert_eq!(form.github, "acme/checkout");
                assert!(form.clone_url.contains("acme/widget.git"));
                assert!(!github::commits_preview(&form.github).contains("--mirror"));
            }
            _ => panic!("sync commits did not open"),
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::SyncCommits {
                        github,
                        clone_url,
                        visibility,
                        ..
                    },
                ] => {
                    assert_eq!(github, "acme/checkout");
                    assert_eq!(clone_url, "https://origin.cursor.com/acme/widget.git");
                    assert_eq!(visibility, "private");
                }
                _ => panic!("expected a commit sync"),
            },
            _ => panic!("expected a commit sync"),
        }
    }

    #[test]
    fn u_without_a_child_does_not_push() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        match app.on_key(key('u')) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("a sync was started"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.text.contains("no GitHub child"), "{}", toast.text);
    }

    #[test]
    fn synced_commits_ask_for_a_fresh_status() {
        let mut repo = sample("acme", "widget");
        repo.github_child = Some("acme/checkout".to_string());
        repo.branches = vec![Branch {
            name: "main".to_string(),
            sha: "abc".to_string(),
        }];
        let mut app = fixture(vec![repo]);
        app.github_generation = 2;
        app.branches_inflight = false;
        let jobs = app.on_msg(Msg::CommitsSynced {
            generation: 2,
            result: Ok(()),
        });
        assert_eq!(
            app.toast.as_ref().map(|toast| toast.text.as_str()),
            Some("Synced commits to GitHub")
        );
        assert!(jobs.iter().any(|job| matches!(
            job,
            Job::ChildStatus { github, .. } if github == "acme/checkout"
        )));
    }

    #[test]
    fn enter_opens_the_readme_and_a_missing_file_stays_hidden() {
        let mut repo = sample("acme", "widget");
        repo.readme = Some(crate::model::Readme {
            path: "README.md".to_string(),
            text: "# Hi\n".to_string(),
        });
        let mut app = fixture(vec![repo]);
        app.focus = Focus::Detail;
        app.selected_field = FieldId::Readme;
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("viewing fetched again"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::Readme(view)) => {
                assert_eq!(view.full_name, "acme/widget");
                assert_eq!(view.path, "README.md");
                assert_eq!(view.text, "# Hi\n");
            }
            _ => panic!("readme did not open"),
        }
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "# Hi\n");
                assert_eq!(toast, "Copied README");
            }
            _ => panic!("expected copy"),
        }
        app.overlay = None;
        app.readme_generation = 3;
        app.on_msg(Msg::Readme {
            generation: 3,
            full_name: "acme/widget".to_string(),
            branch: "main".to_string(),
            result: Ok(origin::ReadmeFetch::Missing),
        });
        assert!(app.repos[0].readme.is_none());
        assert!(
            app.repos[0]
                .fields()
                .iter()
                .all(|field| field.id != FieldId::Readme)
        );
        app.on_msg(Msg::Readme {
            generation: 2,
            full_name: "acme/widget".to_string(),
            branch: "main".to_string(),
            result: Ok(origin::ReadmeFetch::Found(crate::model::Readme {
                path: "README.md".to_string(),
                text: "stale".to_string(),
            })),
        });
        assert!(app.repos[0].readme.is_none());
    }

    #[test]
    fn y_copies_the_github_child_url() {
        let mut repo = sample("acme", "widget");
        repo.github_child = Some("acme/checkout".to_string());
        let mut app = fixture(vec![repo]);
        app.focus = Focus::Detail;
        app.selected_field = FieldId::GithubChild;
        match app.on_key(key('y')) {
            Effect::Copy { text, toast } => {
                assert_eq!(text, "https://github.com/acme/checkout");
                assert_eq!(toast, "Copied github child");
            }
            _ => panic!("expected copy"),
        }
    }

    #[test]
    fn d_requires_the_repository_name() {
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.on_key(key('d'));
        match &app.overlay {
            Some(Overlay::Delete(form)) => assert_eq!(form.full_name, "acme/widget"),
            _ => panic!("delete form did not open"),
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("deleted without the name"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::Delete(form)) => assert!(form.error.is_some()),
            _ => panic!("form closed"),
        }
        for ch in "acme/widget".chars() {
            app.on_key(key(ch));
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [Job::Delete { full_name, .. }] => assert_eq!(full_name, "acme/widget"),
                _ => panic!("expected a delete job"),
            },
            _ => panic!("expected a delete job"),
        }
    }

    #[test]
    fn deleted_repo_drops_its_github_child() {
        let path =
            std::env::temp_dir().join(format!("ot-children-{}-delete.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut app = fixture(vec![sample("acme", "widget")]);
        app.children_path = path.clone();
        app.children.insert(
            "acme/widget".to_string(),
            Child {
                github: "acme/checkout".to_string(),
                visibility: "private".to_string(),
            },
        );
        app.delete_generation = 1;
        let jobs = app.on_msg(Msg::Deleted {
            generation: 1,
            full_name: "acme/widget".to_string(),
            result: Ok(()),
        });
        assert!(matches!(jobs.first(), Some(Job::RepoList { .. })));
        assert!(app.overlay.is_none());
        assert!(app.children.is_empty());
        assert!(github::load_children(&path).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    fn two_branches(default_name: &str) -> Repo {
        let mut repo = sample("acme", "widget");
        repo.view = Some(ViewRepo {
            default_branch: Some(default_name.to_string()),
            mirror_status: Some("no-mirror".to_string()),
            ..ViewRepo::default()
        });
        repo.branches = vec![branch(default_name), branch("topic/one")];
        repo.commits = vec![commit("abcdef0123456789")];
        repo.commits_for = Some("topic/one".to_string());
        repo
    }

    #[test]
    fn x_requires_the_branch_name() {
        let mut app = fixture(vec![two_branches("main")]);
        app.focus = Focus::Detail;
        app.cursor = DetailCursor::Branch(1);
        app.selected_branch = Some("topic/one".to_string());
        app.branch_locked = true;
        app.on_key(key('d'));
        match &app.overlay {
            Some(Overlay::Delete(form)) => assert_eq!(form.full_name, "acme/widget"),
            _ => panic!("d should still delete the repository"),
        }
        app.overlay = None;
        app.on_key(key('x'));
        match &app.overlay {
            Some(Overlay::DeleteBranch(form)) => {
                assert_eq!(form.full_name, "acme/widget");
                assert_eq!(form.branch, "topic/one");
            }
            _ => panic!("branch delete form did not open"),
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("deleted without the branch name"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        match &app.overlay {
            Some(Overlay::DeleteBranch(form)) => {
                assert_eq!(form.error.as_deref(), Some("Type topic/one to confirm."));
            }
            _ => panic!("form closed"),
        }
        for ch in "topic".chars() {
            app.on_key(key(ch));
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::None => {}
            Effect::Jobs(_) => panic!("a prefix confirmed the delete"),
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for ch in "topic/one".chars() {
            app.on_key(key(ch));
        }
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Effect::Jobs(jobs) => match &jobs[..] {
                [
                    Job::DeleteBranch {
                        full_name, branch, ..
                    },
                ] => {
                    assert_eq!(full_name, "acme/widget");
                    assert_eq!(branch, "topic/one");
                }
                _ => panic!("expected a branch delete job"),
            },
            _ => panic!("expected a branch delete job"),
        }
    }

    #[test]
    fn x_refuses_the_default_and_a_mirror() {
        let mut app = fixture(vec![two_branches("main")]);
        app.focus = Focus::Detail;
        app.cursor = DetailCursor::Branch(0);
        app.on_key(key('x'));
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.text.contains("default"), "{}", toast.text);

        let mut mirrored = two_branches("main");
        mirrored.view.as_mut().unwrap().mirror_status = Some("inbound".to_string());
        let mut app = fixture(vec![mirrored]);
        app.focus = Focus::Detail;
        app.cursor = DetailCursor::Branch(1);
        app.detail_inflight = true;
        app.on_key(key('x'));
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(
            toast.text.contains("GitHub is the source"),
            "{}",
            toast.text
        );

        let mut loading = sample("acme", "widget");
        loading.branches = vec![branch("main"), branch("topic/one")];
        let mut app = fixture(vec![loading]);
        app.focus = Focus::Detail;
        app.cursor = DetailCursor::Branch(1);
        app.detail_inflight = true;
        app.on_key(key('x'));
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.text.contains("still loading"), "{}", toast.text);

        let mut app = fixture(vec![two_branches("main")]);
        app.on_key(key('x'));
        assert!(app.overlay.is_none());
        assert!(app.toast.is_none());
        app.focus = Focus::Detail;
        app.on_key(key('x'));
        assert!(app.overlay.is_none());
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.text.contains("Select a branch"), "{}", toast.text);
    }

    #[test]
    fn deleted_branch_reloads_and_falls_back_to_the_default() {
        let mut app = fixture(vec![two_branches("main")]);
        app.focus = Focus::Detail;
        app.cursor = DetailCursor::Branch(1);
        app.selected_branch = Some("topic/one".to_string());
        app.branch_locked = true;
        app.detail_generation = 3;
        app.branch_delete_generation = 1;
        let jobs = app.on_msg(Msg::BranchDeleted {
            generation: 1,
            full_name: "acme/widget".to_string(),
            branch: "topic/one".to_string(),
            result: Ok(()),
        });
        assert!(app.overlay.is_none());
        assert_eq!(app.toast.as_ref().unwrap().text, "Branch deleted");
        assert!(
            app.repos[0]
                .branches
                .iter()
                .all(|item| item.name != "topic/one")
        );
        assert_eq!(app.selected_branch.as_deref(), Some("main"));
        assert!(matches!(app.cursor, DetailCursor::Branch(0)));
        assert!(app.repos[0].commits.is_empty());
        assert!(
            jobs.iter()
                .any(|job| matches!(job, Job::Commits { branch, .. } if branch == "main"))
        );
        assert!(jobs.iter().any(|job| matches!(
            job,
            Job::Branches {
                full_name,
                generation,
            } if full_name == "acme/widget" && *generation == 4
        )));
        assert!(
            jobs.iter()
                .any(|job| matches!(job, Job::Pulls { generation, .. } if *generation == 4))
        );
        app.repos[0].branches = vec![branch("main"), branch("topic/one")];
        app.branch_delete_generation = 2;
        let jobs = app.on_msg(Msg::BranchDeleted {
            generation: 1,
            full_name: "acme/widget".to_string(),
            branch: "topic/one".to_string(),
            result: Ok(()),
        });
        assert!(jobs.is_empty());
        assert_eq!(app.repos[0].branches.len(), 2);
        assert_eq!(app.detail_generation, 4);
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
            Effect::Jobs(jobs) if jobs.iter().any(|job| matches!(job, Job::Commits { .. })) => {
                panic!("the loaded branch was fetched again")
            }
            Effect::Jobs(_) => {}
            Effect::Quit => panic!("quit"),
            Effect::Copy { .. } => panic!("copy"),
        }
        assert!(matches!(app.cursor, DetailCursor::Branch(0)));
        match app.on_key(key('j')) {
            Effect::Jobs(jobs) => {
                assert!(jobs.iter().any(|job| matches!(
                    job,
                    Job::Commits { full_name, branch, .. }
                        if full_name == "acme/widget" && branch == "topic"
                )));
                assert!(jobs.iter().any(|job| matches!(
                    job,
                    Job::Readme { branch, .. } if branch == "topic"
                )));
            }
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
        assert!(
            jobs.iter()
                .any(|job| matches!(job, Job::Commits { branch, .. } if branch == "develop")),
            "expected the first branch before a default is known"
        );
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
        assert!(
            jobs.iter()
                .any(|job| matches!(job, Job::Commits { branch, .. } if branch == "main")),
            "expected commits for the default branch"
        );
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
        assert!(
            jobs.iter()
                .any(|job| matches!(job, Job::Commits { branch, .. } if branch == "develop")),
            "a chosen branch should stay selected"
        );
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
