use crate::text::format_time;

#[derive(Clone, Debug, Default)]
pub struct AuthStatus {
    pub account: Option<String>,
    pub method: Option<String>,
    pub endpoint: Option<String>,
    pub token: Option<String>,
}

impl AuthStatus {
    pub fn signed_in(&self) -> bool {
        self.token.as_deref() == Some("valid") && self.account.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct Namespace {
    pub slug: String,
    pub kind: Option<String>,
    pub can_create: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ApiRepo {
    pub id: Option<String>,
    pub full_name: String,
    pub default_branch: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub pushed_at: Option<String>,
    pub owner_slug: Option<String>,
    pub owner_kind: Option<String>,
    pub owner_id: Option<String>,
    pub clone_url: Option<String>,
    pub web_url: Option<String>,
    pub visibility: Option<String>,
    pub mirror_source: Option<String>,
    pub mirror_status: Option<String>,
    pub allow_merge_commit: Option<bool>,
    pub allow_squash_merge: Option<bool>,
    pub delete_branch_on_merge: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct ViewRepo {
    pub default_branch: Option<String>,
    pub repo_uuid: Option<String>,
    pub mirror_status: Option<String>,
    pub forge_environment: Option<String>,
    pub origin_namespace_id: Option<String>,
    pub github_node_id: Option<String>,
    pub github_enterprise_id: Option<String>,
    pub github_installation_id: Option<String>,
    pub active_mirror_transition_job_id: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub last_pushed_at: Option<String>,
    pub clone_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ListedRepo {
    pub owner: String,
    pub name: String,
    pub clone_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub status: String,
    pub url: String,
}

pub const PULL_LIMIT: usize = 50;

pub const COMMIT_LIMIT: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub summary: String,
    pub author: String,
    pub date: String,
    pub url: String,
}

pub const VISIBILITIES: [&str; 2] = ["private", "internal"];

#[derive(Clone, Debug)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub listed_clone: String,
    pub summary: Option<ApiRepo>,
    pub fetched: Option<ApiRepo>,
    pub view: Option<ViewRepo>,
    pub detail_error: Option<String>,
    pub pulls: Vec<PullRequest>,
    pub pulls_error: Option<String>,
    pub branches: Vec<Branch>,
    pub branches_error: Option<String>,
    pub commits: Vec<Commit>,
    pub commits_error: Option<String>,
    pub commits_for: Option<String>,
}

impl Repo {
    pub fn from_listed(listed: ListedRepo) -> Self {
        let full_name = format!("{}/{}", listed.owner, listed.name);
        Self {
            owner: listed.owner,
            name: listed.name,
            full_name,
            listed_clone: listed.clone_url,
            summary: None,
            fetched: None,
            view: None,
            detail_error: None,
            pulls: Vec::new(),
            pulls_error: None,
            branches: Vec::new(),
            branches_error: None,
            commits: Vec::new(),
            commits_error: None,
            commits_for: None,
        }
    }

    pub fn default_branch_name(&self) -> Option<String> {
        self.view
            .as_ref()
            .and_then(|view| view.default_branch.clone())
            .or_else(|| self.api().and_then(|api| api.default_branch.clone()))
    }

    pub(crate) fn api(&self) -> Option<&ApiRepo> {
        self.fetched.as_ref().or(self.summary.as_ref())
    }

    pub fn clone_url(&self) -> Option<&str> {
        self.view
            .as_ref()
            .and_then(|view| view.clone_url.as_deref())
            .filter(|url| !url.is_empty())
            .or(if self.listed_clone.is_empty() {
                None
            } else {
                Some(self.listed_clone.as_str())
            })
            .or_else(|| self.api().and_then(|api| api.clone_url.as_deref()))
    }

    pub fn web_url(&self) -> Option<&str> {
        self.api().and_then(|api| api.web_url.as_deref())
    }

    pub fn mirrored(&self) -> bool {
        if let Some(view) = &self.view {
            return matches!(
                view.mirror_status.as_deref(),
                Some(status) if !status.is_empty() && status != "no-mirror"
            );
        }
        match self.api() {
            Some(api) => match (api.mirror_status.as_deref(), api.mirror_source.as_deref()) {
                (Some(status), _) if !status.is_empty() && status != "no-mirror" => true,
                (_, Some(source)) if !source.is_empty() => true,
                _ => false,
            },
            None => false,
        }
    }

    pub fn badge(&self) -> Option<&'static str> {
        self.mirrored().then_some("m")
    }

    pub fn visibility(&self) -> Option<RepoVisibility> {
        match self.api().and_then(|api| api.visibility.as_deref()) {
            Some("public") => Some(RepoVisibility::Public),
            Some("private") => Some(RepoVisibility::Private),
            Some("internal") => Some(RepoVisibility::Internal),
            _ => None,
        }
    }

    pub fn apply_summary(&mut self, api: ApiRepo) {
        if self.listed_clone.is_empty()
            && let Some(url) = api.clone_url.clone()
        {
            self.listed_clone = url;
        }
        self.summary = Some(api);
    }

    pub fn apply_detail(
        &mut self,
        view: Result<ViewRepo, String>,
        fetched: Result<ApiRepo, String>,
    ) {
        match (view, fetched) {
            (Ok(view), Ok(api)) => {
                if self.listed_clone.is_empty()
                    && let Some(url) = view.clone_url.clone().or(api.clone_url.clone())
                {
                    self.listed_clone = url;
                }
                self.view = Some(view);
                self.fetched = Some(api);
                self.detail_error = None;
            }
            (Ok(view), Err(_)) => {
                if self.listed_clone.is_empty()
                    && let Some(url) = view.clone_url.clone()
                {
                    self.listed_clone = url;
                }
                self.view = Some(view);
                self.detail_error = None;
            }
            (Err(_), Ok(api)) => {
                if self.listed_clone.is_empty()
                    && let Some(url) = api.clone_url.clone()
                {
                    self.listed_clone = url;
                }
                self.fetched = Some(api);
                self.detail_error = None;
            }
            (Err(view_err), Err(api_err)) => {
                self.detail_error = Some(if view_err == api_err {
                    view_err
                } else {
                    format!("{view_err}; {api_err}")
                });
            }
        }
    }

    pub fn apply_settings(&mut self, api: ApiRepo) {
        if let Some(branch) = api.default_branch.clone()
            && let Some(view) = &mut self.view
        {
            view.default_branch = Some(branch);
        }
        if let Some(dst) = self.fetched.as_mut() {
            merge_api(dst, api);
        } else if let Some(dst) = self.summary.as_mut() {
            merge_api(dst, api);
        } else {
            self.fetched = Some(api);
        }
    }

    pub fn fields(&self) -> Vec<Field> {
        let mut fields = Vec::new();
        fields.push(Field::same(FieldId::Name, "name", self.full_name.clone()));
        if let Some(visibility) = self.api().and_then(|api| api.visibility.clone()) {
            fields.push(Field::same(FieldId::Visibility, "visibility", visibility));
        }
        if let Some(branch) = self
            .view
            .as_ref()
            .and_then(|view| view.default_branch.clone())
            .or_else(|| self.api().and_then(|api| api.default_branch.clone()))
        {
            fields.push(Field::same(FieldId::Branch, "default branch", branch));
        }
        if let Some(mirror) = mirror_field(self) {
            fields.push(mirror);
        }
        if let Some(forge) = self
            .view
            .as_ref()
            .and_then(|view| view.forge_environment.clone())
        {
            fields.push(Field::same(FieldId::Forge, "forge", forge));
        }
        let slug = self
            .api()
            .and_then(|api| api.owner_slug.clone())
            .unwrap_or_else(|| self.owner.clone());
        let owner = match self.api().and_then(|api| api.owner_kind.clone()) {
            Some(kind) => format!("{slug} · {kind}"),
            None => slug,
        };
        fields.push(Field::same(FieldId::Owner, "owner", owner));
        push_time(
            &mut fields,
            FieldId::Created,
            "created",
            self.view
                .as_ref()
                .and_then(|view| view.created_at.clone())
                .or_else(|| self.api().and_then(|api| api.created_at.clone())),
        );
        push_time(
            &mut fields,
            FieldId::Updated,
            "updated",
            self.view
                .as_ref()
                .and_then(|view| view.updated_at.clone())
                .or_else(|| self.api().and_then(|api| api.updated_at.clone())),
        );
        push_time(
            &mut fields,
            FieldId::Pushed,
            "last push",
            self.view
                .as_ref()
                .and_then(|view| view.last_pushed_at.clone())
                .or_else(|| self.api().and_then(|api| api.pushed_at.clone())),
        );
        if let Some(url) = self.clone_url() {
            fields.push(Field::same(FieldId::Clone, "clone", url.to_string()));
        }
        if let Some(url) = self.web_url() {
            fields.push(Field::same(FieldId::Web, "web", url.to_string()));
        }
        if let Some(api) = self.api() {
            push_flag(
                &mut fields,
                FieldId::MergeCommit,
                "merge commits",
                api.allow_merge_commit,
            );
            push_flag(
                &mut fields,
                FieldId::Squash,
                "squash",
                api.allow_squash_merge,
            );
            push_flag(
                &mut fields,
                FieldId::DeleteBranch,
                "delete branch",
                api.delete_branch_on_merge,
            );
        }
        if let Some(id) = self
            .view
            .as_ref()
            .and_then(|view| view.repo_uuid.clone())
            .or_else(|| self.api().and_then(|api| api.id.clone()))
        {
            fields.push(Field::same(FieldId::Id, "id", id));
        }
        if let Some(id) = self
            .view
            .as_ref()
            .and_then(|view| view.origin_namespace_id.clone())
            .or_else(|| self.api().and_then(|api| api.owner_id.clone()))
        {
            fields.push(Field::same(FieldId::Namespace, "namespace", id));
        }
        if let Some(github) = github_line(self) {
            fields.push(Field::same(FieldId::Github, "github", github));
        }
        if let Some(job) = self
            .view
            .as_ref()
            .and_then(|view| view.active_mirror_transition_job_id.clone())
        {
            fields.push(Field::same(FieldId::Transition, "transition", job));
        }
        fields
    }
}

fn push_time(fields: &mut Vec<Field>, id: FieldId, label: &'static str, raw: Option<String>) {
    if let Some(raw) = raw {
        fields.push(Field {
            id,
            label,
            display: format_time(&raw),
            copy: raw,
        });
    }
}

fn mirror_field(repo: &Repo) -> Option<Field> {
    let known = repo.view.is_some() || repo.api().is_some();
    if !known {
        return None;
    }
    let status = repo
        .view
        .as_ref()
        .and_then(|view| view.mirror_status.clone())
        .or_else(|| repo.api().and_then(|api| api.mirror_status.clone()));
    let source = repo.api().and_then(|api| api.mirror_source.clone());
    let origin_native = match status.as_deref() {
        Some("no-mirror") => true,
        None => source.is_none(),
        Some(_) => false,
    };
    if origin_native {
        return Some(Field::new(
            FieldId::Mirror,
            "mirror",
            "origin".to_string(),
            "no-mirror".to_string(),
        ));
    }
    let display = match (status.as_deref(), source.as_deref()) {
        (Some(status), Some(source)) => format!("{status} · {source}"),
        (Some(status), None) => status.to_string(),
        (None, Some(source)) => source.to_string(),
        (None, None) => "mirrored".to_string(),
    };
    Some(Field::same(FieldId::Mirror, "mirror", display))
}

fn push_flag(fields: &mut Vec<Field>, id: FieldId, label: &'static str, value: Option<bool>) {
    if let Some(on) = value {
        fields.push(Field::same(id, label, on_off(on).to_string()));
    }
}

pub fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn merge_api(dst: &mut ApiRepo, src: ApiRepo) {
    if src.id.is_some() {
        dst.id = src.id;
    }
    if !src.full_name.is_empty() {
        dst.full_name = src.full_name;
    }
    keep(&mut dst.default_branch, src.default_branch);
    keep(&mut dst.created_at, src.created_at);
    keep(&mut dst.updated_at, src.updated_at);
    keep(&mut dst.pushed_at, src.pushed_at);
    keep(&mut dst.owner_slug, src.owner_slug);
    keep(&mut dst.owner_kind, src.owner_kind);
    keep(&mut dst.owner_id, src.owner_id);
    keep(&mut dst.clone_url, src.clone_url);
    keep(&mut dst.web_url, src.web_url);
    keep(&mut dst.visibility, src.visibility);
    keep(&mut dst.mirror_source, src.mirror_source);
    keep(&mut dst.mirror_status, src.mirror_status);
    keep(&mut dst.allow_merge_commit, src.allow_merge_commit);
    keep(&mut dst.allow_squash_merge, src.allow_squash_merge);
    keep(&mut dst.delete_branch_on_merge, src.delete_branch_on_merge);
}

fn keep<T>(dst: &mut Option<T>, src: Option<T>) {
    if let Some(value) = src {
        *dst = Some(value);
    }
}

fn github_line(repo: &Repo) -> Option<String> {
    let view = repo.view.as_ref()?;
    let mut parts = Vec::new();
    if let Some(id) = &view.github_node_id {
        parts.push(format!("node {id}"));
    }
    if let Some(id) = &view.github_enterprise_id {
        parts.push(format!("enterprise {id}"));
    }
    if let Some(id) = &view.github_installation_id {
        parts.push(format!("installation {id}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldId {
    Name,
    Visibility,
    Branch,
    Mirror,
    Forge,
    Owner,
    Created,
    Updated,
    Pushed,
    Clone,
    Web,
    MergeCommit,
    Squash,
    DeleteBranch,
    Id,
    Namespace,
    Github,
    Transition,
}

impl FieldId {
    pub fn editable(self) -> bool {
        matches!(
            self,
            Self::Visibility | Self::Branch | Self::MergeCommit | Self::Squash | Self::DeleteBranch
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: FieldId,
    pub label: &'static str,
    pub display: String,
    pub copy: String,
}

impl Field {
    pub fn same(id: FieldId, label: &'static str, value: String) -> Self {
        Self::new(id, label, value.clone(), value)
    }

    pub fn new(id: FieldId, label: &'static str, display: String, copy: String) -> Self {
        Self {
            id,
            label,
            display,
            copy,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub value: String,
    pub cursor: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new("")
    }
}

impl Editor {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn insert_str(&mut self, text: &str) {
        let clean: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let room = 300usize.saturating_sub(self.value.len());
        let clean: String = clean.chars().take(room).collect();
        if clean.is_empty() {
            return;
        }
        self.value.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    pub fn insert_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.insert_str(&ch.to_string());
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = prev_boundary(&self.value, self.cursor);
        self.value.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let next = next_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..next, "");
    }

    pub fn move_left(&mut self) {
        self.cursor = prev_boundary(&self.value, self.cursor);
    }

    pub fn move_right(&mut self) {
        self.cursor = next_boundary(&self.value, self.cursor);
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub fn kill_to_start(&mut self) {
        self.value.replace_range(0..self.cursor, "");
        self.cursor = 0;
    }

    pub fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before: Vec<(usize, char)> = self.value[..self.cursor].char_indices().collect();
        let mut end = self.cursor;
        let mut i = before.len();
        while i > 0 && before[i - 1].1.is_whitespace() {
            i -= 1;
            end = before[i].0;
        }
        if i == 0 {
            self.value.replace_range(0..self.cursor, "");
            self.cursor = 0;
            return;
        }
        let word = is_editor_word(before[i - 1].1);
        while i > 0 {
            let ch = before[i - 1].1;
            if ch.is_whitespace() || is_editor_word(ch) != word {
                break;
            }
            i -= 1;
            end = before[i].0;
        }
        self.value.replace_range(end..self.cursor, "");
        self.cursor = end;
    }

    pub fn window(&self, width: usize) -> (String, usize) {
        if width == 0 {
            return (String::new(), 0);
        }
        let cursor_col = crate::text::display_width(&self.value[..self.cursor]);
        let start = (cursor_col + 1).saturating_sub(width);
        let total = crate::text::display_width(&self.value);
        let end = (start + width).min(total);
        let visible = crate::text::slice_cols(&self.value, start, end);
        (visible, cursor_col - start)
    }
}

fn is_editor_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

fn prev_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    value[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
        .unwrap_or(cursor)
}

#[derive(Clone, Debug)]
pub struct CreateForm {
    pub name: Editor,
    pub namespace: Editor,
    pub branch: Editor,
    pub focus: usize,
    pub error: Option<String>,
    pub busy: bool,
    pub generation: u64,
}

impl CreateForm {
    pub fn focused_editor(&mut self) -> &mut Editor {
        match self.focus {
            0 => &mut self.name,
            1 => &mut self.namespace,
            _ => &mut self.branch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoPatch {
    Visibility(String),
    Branch(String),
    Merge {
        allow_merge: bool,
        allow_squash: bool,
    },
    DeleteBranch(bool),
}

#[derive(Clone, Debug)]
pub enum EditKind {
    Visibility {
        current: String,
        choice: usize,
    },
    Branch(Editor),
    Confirm {
        label: &'static str,
        from: bool,
        to: bool,
        patch: RepoPatch,
    },
}

#[derive(Clone, Debug)]
pub struct EditForm {
    pub full_name: String,
    pub kind: EditKind,
    pub error: Option<String>,
    pub busy: bool,
    pub generation: u64,
}

impl EditForm {
    pub fn patch(&self) -> RepoPatch {
        match &self.kind {
            EditKind::Visibility { choice, .. } => {
                let value = VISIBILITIES
                    .get(*choice)
                    .copied()
                    .unwrap_or(VISIBILITIES[0]);
                RepoPatch::Visibility(value.to_string())
            }
            EditKind::Branch(editor) => RepoPatch::Branch(editor.value.trim().to_string()),
            EditKind::Confirm { patch, .. } => patch.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTarget {
    Field(FieldId),
    Branch(usize),
    Commit(usize),
    Pull(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailCursor {
    #[default]
    Field,
    Branch(usize),
    Commit(usize),
    Pull(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailSpan {
    pub target: DetailTarget,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailLine {
    DetailError,
    Field { id: FieldId, part: usize },
    Gap,
    BranchesHeading,
    BranchesLoading,
    BranchesError,
    Branch(usize),
    CommitsHeading,
    CommitsLoading,
    CommitsError,
    CommitsEmpty,
    Commit(usize),
    CommitsCapped,
    Heading,
    PullsError,
    Loading,
    Empty,
    Pull(usize),
    Capped,
}

pub fn detail_lines(
    repo: &Repo,
    value_width: usize,
    pulls_inflight: bool,
    branches_inflight: bool,
    commits_inflight: bool,
) -> Vec<DetailLine> {
    if value_width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    if repo.detail_error.is_some() {
        lines.push(DetailLine::DetailError);
    }
    for field in repo.fields() {
        let count = crate::text::wrap_parts(&field.display, value_width)
            .len()
            .max(1);
        for part in 0..count {
            lines.push(DetailLine::Field { id: field.id, part });
        }
    }
    lines.push(DetailLine::Gap);
    push_history(&mut lines, repo, branches_inflight, commits_inflight);
    lines.push(DetailLine::Gap);
    lines.push(DetailLine::Heading);
    if repo.pulls_error.is_some() {
        lines.push(DetailLine::PullsError);
    }
    if repo.pulls.is_empty() {
        if pulls_inflight && repo.pulls_error.is_none() {
            lines.push(DetailLine::Loading);
        } else if repo.pulls_error.is_none() {
            lines.push(DetailLine::Empty);
        }
    } else {
        for index in 0..repo.pulls.len() {
            lines.push(DetailLine::Pull(index));
        }
        if repo.pulls.len() == PULL_LIMIT {
            lines.push(DetailLine::Capped);
        }
    }
    lines
}

fn push_history(
    lines: &mut Vec<DetailLine>,
    repo: &Repo,
    branches_inflight: bool,
    commits_inflight: bool,
) {
    if repo.branches_error.is_some() {
        lines.push(DetailLine::BranchesError);
    }
    if repo.branches.is_empty() {
        if branches_inflight && repo.branches_error.is_none() {
            lines.push(DetailLine::BranchesLoading);
            return;
        }
        if repo.branches_error.is_some() {
            return;
        }
        lines.push(DetailLine::CommitsHeading);
        lines.push(DetailLine::CommitsEmpty);
        return;
    }
    if repo.branches.len() > 1 {
        lines.push(DetailLine::BranchesHeading);
        for index in 0..repo.branches.len() {
            lines.push(DetailLine::Branch(index));
        }
        lines.push(DetailLine::Gap);
    }
    lines.push(DetailLine::CommitsHeading);
    if repo.commits_error.is_some() {
        lines.push(DetailLine::CommitsError);
    }
    if repo.commits.is_empty() {
        if commits_inflight && repo.commits_error.is_none() {
            lines.push(DetailLine::CommitsLoading);
        } else if repo.commits_error.is_none() {
            lines.push(DetailLine::CommitsEmpty);
        }
        return;
    }
    for index in 0..repo.commits.len() {
        lines.push(DetailLine::Commit(index));
    }
    if repo.commits.len() == COMMIT_LIMIT {
        lines.push(DetailLine::CommitsCapped);
    }
}

pub fn order_branches(branches: &mut [Branch], default_name: Option<&str>) {
    branches.sort_by(|left, right| {
        let rank = |name: &str| usize::from(default_name != Some(name));
        rank(&left.name)
            .cmp(&rank(&right.name))
            .then(left.name.cmp(&right.name))
    });
}

pub fn preferred_branch(repo: &Repo) -> Option<String> {
    if let Some(name) = repo.default_branch_name()
        && repo.branches.iter().any(|branch| branch.name == name)
    {
        return Some(name);
    }
    repo.branches.first().map(|branch| branch.name.clone())
}

pub fn spans_from(lines: &[DetailLine]) -> Vec<DetailSpan> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        match lines[index] {
            DetailLine::Field { id, .. } => {
                let start = index;
                index += 1;
                while index < lines.len()
                    && matches!(lines[index], DetailLine::Field { id: next, .. } if next == id)
                {
                    index += 1;
                }
                spans.push(DetailSpan {
                    target: DetailTarget::Field(id),
                    start,
                    end: index,
                });
            }
            DetailLine::Branch(branch) => {
                spans.push(DetailSpan {
                    target: DetailTarget::Branch(branch),
                    start: index,
                    end: index + 1,
                });
                index += 1;
            }
            DetailLine::Commit(commit) => {
                spans.push(DetailSpan {
                    target: DetailTarget::Commit(commit),
                    start: index,
                    end: index + 1,
                });
                index += 1;
            }
            DetailLine::Pull(pull) => {
                spans.push(DetailSpan {
                    target: DetailTarget::Pull(pull),
                    start: index,
                    end: index + 1,
                });
                index += 1;
            }
            _ => index += 1,
        }
    }
    spans
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoVisibility {
    Public,
    Private,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Repo(usize),
}

#[derive(Clone, Debug)]
pub struct DisplayRow {
    pub kind: RowKind,
    pub label: String,
}

pub fn display_rows(repos: &[Repo], visible: &[usize], group: bool) -> Vec<DisplayRow> {
    if !group {
        return visible
            .iter()
            .map(|index| DisplayRow {
                kind: RowKind::Repo(*index),
                label: repos[*index].name.clone(),
            })
            .collect();
    }
    let mut owners: Vec<&str> = Vec::new();
    for index in visible {
        let owner = repos[*index].owner.as_str();
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    let mut rows = Vec::new();
    for owner in owners {
        rows.push(DisplayRow {
            kind: RowKind::Header,
            label: owner.to_string(),
        });
        for index in visible {
            if repos[*index].owner == owner {
                rows.push(DisplayRow {
                    kind: RowKind::Repo(*index),
                    label: repos[*index].name.clone(),
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> Repo {
        Repo::from_listed(ListedRepo {
            owner: owner.to_string(),
            name: name.to_string(),
            clone_url: format!("https://origin.cursor.com/{owner}/{name}.git"),
        })
    }

    #[test]
    fn groups_by_owner_and_keeps_order() {
        let repos = vec![repo("b", "one"), repo("a", "two"), repo("b", "three")];
        let visible = vec![0, 1, 2];
        let rows = display_rows(&repos, &visible, true);
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["b", "one", "three", "a", "two"]);
    }

    #[test]
    fn editor_deletes_a_word() {
        let mut editor = Editor::new("hello world");
        editor.delete_word_back();
        assert_eq!(editor.value, "hello ");
        assert_eq!(editor.cursor, editor.value.len());
    }

    #[test]
    fn window_keeps_the_cursor_in_view() {
        let mut editor = Editor::new("abcdef");
        editor.cursor = 6;
        let (text, col) = editor.window(3);
        assert_eq!(text, "ef");
        assert_eq!(col, 2);
    }

    #[test]
    fn settings_merge_keeps_an_omitted_mirror() {
        let mut repo = repo("acme", "widget");
        repo.view = Some(ViewRepo {
            default_branch: Some("main".to_string()),
            ..ViewRepo::default()
        });
        repo.fetched = Some(ApiRepo {
            full_name: "acme/widget".to_string(),
            visibility: Some("private".to_string()),
            mirror_source: Some("github".to_string()),
            mirror_status: Some("inbound".to_string()),
            allow_merge_commit: Some(true),
            allow_squash_merge: Some(false),
            delete_branch_on_merge: Some(false),
            ..ApiRepo::default()
        });
        repo.apply_settings(ApiRepo {
            full_name: "acme/widget".to_string(),
            visibility: Some("internal".to_string()),
            default_branch: Some("trunk".to_string()),
            ..ApiRepo::default()
        });
        let api = repo.fetched.as_ref().unwrap();
        assert_eq!(api.visibility.as_deref(), Some("internal"));
        assert_eq!(api.mirror_source.as_deref(), Some("github"));
        assert_eq!(api.allow_merge_commit, Some(true));
        assert_eq!(
            repo.view.as_ref().unwrap().default_branch.as_deref(),
            Some("trunk")
        );
        let fields = repo.fields();
        assert!(
            fields
                .iter()
                .any(|field| field.id == FieldId::MergeCommit && field.display == "on")
        );
        assert!(
            fields
                .iter()
                .any(|field| field.id == FieldId::Squash && field.display == "off")
        );
    }

    #[test]
    fn several_branches_come_before_their_commits() {
        let mut repo = repo("acme", "widget");
        repo.view = Some(ViewRepo {
            default_branch: Some("main".to_string()),
            ..ViewRepo::default()
        });
        repo.branches = vec![
            Branch {
                name: "topic".to_string(),
                sha: "bbb".to_string(),
            },
            Branch {
                name: "main".to_string(),
                sha: "aaa".to_string(),
            },
        ];
        let default_name = repo.default_branch_name();
        order_branches(&mut repo.branches, default_name.as_deref());
        assert_eq!(repo.branches[0].name, "main");
        repo.commits = vec![Commit {
            sha: "abcdef0123456789".to_string(),
            summary: "Add gate".to_string(),
            author: "Ada".to_string(),
            date: "2026-10-01T12:00:00Z".to_string(),
            url: "https://example.com/c".to_string(),
        }];
        let lines = detail_lines(&repo, 40, false, false, false);
        let branch_at = lines
            .iter()
            .position(|line| matches!(line, DetailLine::Branch(0)))
            .unwrap();
        let commit_at = lines
            .iter()
            .position(|line| matches!(line, DetailLine::Commit(0)))
            .unwrap();
        assert!(branch_at < commit_at);
        repo.branches.pop();
        let lines = detail_lines(&repo, 40, false, false, false);
        assert!(
            lines
                .iter()
                .all(|line| !matches!(line, DetailLine::Branch(_)))
        );
        assert!(
            lines
                .iter()
                .any(|line| matches!(line, DetailLine::CommitsHeading))
        );
    }

    #[test]
    fn visibility_reads_public_private_and_internal() {
        let mut repo = repo("acme", "widget");
        assert_eq!(repo.visibility(), None);
        for (raw, kind) in [
            ("public", RepoVisibility::Public),
            ("private", RepoVisibility::Private),
            ("internal", RepoVisibility::Internal),
        ] {
            repo.summary = Some(ApiRepo {
                full_name: "acme/widget".to_string(),
                visibility: Some(raw.to_string()),
                ..ApiRepo::default()
            });
            assert_eq!(repo.visibility(), Some(kind));
        }
    }
}
