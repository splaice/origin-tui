use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::model::{
    ApiRepo, AuthStatus, Branch, COMMIT_LIMIT, Commit, ListedRepo, Namespace, PULL_LIMIT,
    PullRequest, RepoPatch, ViewRepo,
};
use crate::text::strip_ansi;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);

pub fn resolve() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ORIGIN_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(path) = which("origin") {
        return Ok(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let fallback = PathBuf::from(home).join(".local/bin/origin");
        if fallback.is_file() {
            return Ok(fallback);
        }
    }
    Err("origin CLI not found. Install it with: curl -fsSL https://downloads.cursor.com/origin/install.sh | sh".into())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn run<I, S>(bin: &Path, args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = Command::new(bin);
    command
        .args(args.into_iter().map(|arg| arg.as_ref().to_string()))
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to run {}: {err}", bin.display()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "origin returned no stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "origin returned no stderr".to_string())?;
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= COMMAND_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("origin timed out".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(format!("origin failed: {err}")),
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        Err(command_error(&stdout, &stderr))
    }
}

fn command_error(stdout: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    let raw = if !stderr.is_empty() { stderr } else { stdout };
    if raw.is_empty() {
        return "origin command failed".to_string();
    }
    let mut lines = raw.lines().take(6).collect::<Vec<_>>().join(" ");
    if lines.chars().count() > 280 {
        lines = lines.chars().take(280).collect();
        lines.push('…');
    }
    lines
}

pub fn create_args(namespace: &str, name: &str, branch: &str) -> Vec<String> {
    let repo = if namespace.is_empty() {
        name.to_string()
    } else {
        format!("{namespace}/{name}")
    };
    let mut args = vec!["repo".to_string(), "create".to_string(), repo];
    if !branch.is_empty() {
        args.push("--default-branch".to_string());
        args.push(branch.to_string());
    }
    args
}

pub fn create_preview(namespace: &str, name: &str, branch: &str) -> String {
    if name.is_empty() {
        return "origin repo create <name>".to_string();
    }
    format!("origin {}", create_args(namespace, name, branch).join(" "))
}

pub fn auth_status(bin: &Path) -> Result<AuthStatus, String> {
    parse_auth_status(&run(bin, ["auth", "status"])?)
}

pub fn repo_list(bin: &Path) -> Result<Vec<ListedRepo>, String> {
    Ok(parse_repo_list(&run(bin, ["repo", "list"])?))
}

pub fn repo_view(bin: &Path, full_name: &str) -> Result<ViewRepo, String> {
    let json = run(
        bin,
        [
            "repo",
            "view",
            full_name,
            "--json",
            "org,name,defaultBranch,repoUuid,mirrorStatus,forgeEnvironment,originNamespaceId,githubNodeId,githubEnterpriseId,githubInstallationId,activeMirrorTransitionJobId,createdAt,updatedAt,lastPushedAt,cloneUrl",
        ],
    )?;
    parse_view(&json)
}

pub fn namespaces(bin: &Path) -> Result<Vec<Namespace>, String> {
    let mut token = String::new();
    let mut all = Vec::new();
    for _ in 0..20 {
        let path = if token.is_empty() {
            "/namespaces?pageSize=100".to_string()
        } else {
            format!("/namespaces?pageSize=100&pageToken={}", encode_seg(&token))
        };
        let body = run(bin, ["api", &path])?;
        let (page, next) = parse_namespaces(&body)?;
        all.extend(page);
        if next.is_empty() || next == token {
            break;
        }
        token = next;
    }
    Ok(all)
}

pub fn list_owner_repos(bin: &Path, owner: &str) -> Result<Vec<ApiRepo>, String> {
    let mut token = String::new();
    let mut all = Vec::new();
    for _ in 0..20 {
        let path = if token.is_empty() {
            format!("/repos/{}?pageSize=100", encode_seg(owner))
        } else {
            format!(
                "/repos/{}?pageSize=100&pageToken={}",
                encode_seg(owner),
                encode_seg(&token)
            )
        };
        let body = run(bin, ["api", &path])?;
        let (page, next) = parse_repo_page(&body)?;
        all.extend(page);
        if next.is_empty() || next == token {
            break;
        }
        token = next;
    }
    Ok(all)
}

pub fn get_repo(bin: &Path, full_name: &str) -> Result<ApiRepo, String> {
    let (owner, name) =
        split_full_name(full_name).ok_or_else(|| format!("expected org/name, got {full_name}"))?;
    let path = format!("/repos/{}/{}", encode_seg(owner), encode_seg(name));
    let body = run(bin, ["api", &path])?;
    parse_api_repo(&body)
}

pub fn create_repo(bin: &Path, namespace: &str, name: &str, branch: &str) -> Result<(), String> {
    let args = create_args(namespace, name, branch);
    let _ = run(bin, args)?;
    Ok(())
}

pub fn pull_args(full_name: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "list".to_string(),
        "-R".to_string(),
        full_name.to_string(),
        "-s".to_string(),
        "all".to_string(),
        "--limit".to_string(),
        PULL_LIMIT.to_string(),
        "--json".to_string(),
        "number,title,status,url,headRef,baseRef,author,updatedAt".to_string(),
        "--color".to_string(),
        "never".to_string(),
    ]
}

pub fn list_pulls(bin: &Path, full_name: &str) -> Result<Vec<PullRequest>, String> {
    parse_pulls(&run(bin, pull_args(full_name))?)
}

pub fn list_branches(bin: &Path, full_name: &str) -> Result<Vec<Branch>, String> {
    let (owner, name) = repo_parts(full_name)?;
    let mut token = String::new();
    let mut all = Vec::new();
    for _ in 0..10 {
        let path = if token.is_empty() {
            format!("/repos/{owner}/{name}/branches?pageSize=100")
        } else {
            format!(
                "/repos/{owner}/{name}/branches?pageSize=100&pageToken={}",
                encode_seg(&token)
            )
        };
        let (page, next) = parse_branches(&run(bin, ["api", &path])?)?;
        all.extend(page);
        if next.is_empty() || next == token {
            break;
        }
        token = next;
    }
    Ok(all)
}

pub fn commits_path(full_name: &str, branch: &str) -> Result<String, String> {
    let (owner, name) = repo_parts(full_name)?;
    Ok(format!(
        "/repos/{owner}/{name}/commits?pageSize={COMMIT_LIMIT}&sha={}",
        encode_seg(branch)
    ))
}

pub fn list_commits(bin: &Path, full_name: &str, branch: &str) -> Result<Vec<Commit>, String> {
    let path = commits_path(full_name, branch)?;
    parse_commits(&run(bin, ["api", &path])?)
}

fn repo_parts(full_name: &str) -> Result<(String, String), String> {
    let (owner, name) =
        split_full_name(full_name).ok_or_else(|| format!("expected org/name, got {full_name}"))?;
    Ok((encode_seg(owner), encode_seg(name)))
}

pub fn update_args(full_name: &str, patch: &RepoPatch) -> Result<Vec<String>, String> {
    let (owner, name) =
        split_full_name(full_name).ok_or_else(|| format!("expected org/name, got {full_name}"))?;
    if matches!(
        patch,
        RepoPatch::Merge {
            allow_merge: false,
            allow_squash: false
        }
    ) {
        return Err("Keep merge commits or squash merges".to_string());
    }
    let path = format!("/repos/{}/{}", encode_seg(owner), encode_seg(name));
    let mut args = vec![
        "api".to_string(),
        "-X".to_string(),
        "PATCH".to_string(),
        path,
    ];
    match patch {
        RepoPatch::Visibility(value) => {
            args.push("-f".to_string());
            args.push(format!("visibility={value}"));
        }
        RepoPatch::Branch(value) => {
            args.push("-f".to_string());
            args.push(format!("defaultBranch={value}"));
        }
        RepoPatch::Merge {
            allow_merge,
            allow_squash,
        } => {
            args.push("-F".to_string());
            args.push(format!("allowMergeCommit={}", bool_arg(*allow_merge)));
            args.push("-F".to_string());
            args.push(format!("allowSquashMerge={}", bool_arg(*allow_squash)));
        }
        RepoPatch::DeleteBranch(value) => {
            args.push("-F".to_string());
            args.push(format!("deleteBranchOnMerge={}", bool_arg(*value)));
        }
    }
    Ok(args)
}

pub fn update_preview(full_name: &str, patch: &RepoPatch) -> String {
    match update_args(full_name, patch) {
        Ok(args) => format!("origin {}", args.join(" ")),
        Err(err) => err,
    }
}

pub fn update_repo(bin: &Path, full_name: &str, patch: &RepoPatch) -> Result<ApiRepo, String> {
    parse_api_repo(&run(bin, update_args(full_name, patch)?)?)
}

fn bool_arg(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn encode_seg(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn parse_auth_status(text: &str) -> Result<AuthStatus, String> {
    let text = strip_ansi(text);
    let mut status = AuthStatus::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "account" => status.account = Some(value.to_string()),
            "auth method" => status.method = Some(value.to_string()),
            "endpoint" => status.endpoint = Some(value.to_string()),
            "token" => status.token = Some(value.to_string()),
            _ => {}
        }
    }
    if status.account.is_none() && status.token.is_none() {
        return Err(command_error(text.trim(), ""));
    }
    Ok(status)
}

pub fn parse_repo_list(text: &str) -> Vec<ListedRepo> {
    let text = strip_ansi(text);
    let mut out = Vec::new();
    let mut current: Option<ListedRepo> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            if let Some(repo) = current.as_mut()
                && repo.clone_url.is_empty()
            {
                repo.clone_url = line.trim().to_string();
            }
            continue;
        }
        if let Some(repo) = current.take() {
            out.push(repo);
        }
        let trimmed = line.trim();
        if let Some((owner, name)) = split_full_name(trimmed) {
            current = Some(ListedRepo {
                owner: owner.to_string(),
                name: name.to_string(),
                clone_url: String::new(),
            });
        }
    }
    if let Some(repo) = current {
        out.push(repo);
    }
    out
}

pub fn split_full_name(value: &str) -> Option<(&str, &str)> {
    let (owner, name) = value.split_once('/')?;
    if owner.is_empty() || name.is_empty() || owner.contains(' ') || name.contains(' ') {
        return None;
    }
    Some((owner, name))
}

pub fn parse_view(text: &str) -> Result<ViewRepo, String> {
    let value = serde_json::from_str::<Value>(text.trim())
        .map_err(|err| format!("bad repo view: {err}"))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    Ok(ViewRepo {
        default_branch: opt_string(&value, "defaultBranch"),
        repo_uuid: opt_string(&value, "repoUuid"),
        mirror_status: opt_string(&value, "mirrorStatus"),
        forge_environment: opt_string(&value, "forgeEnvironment"),
        origin_namespace_id: opt_string(&value, "originNamespaceId"),
        github_node_id: opt_string(&value, "githubNodeId"),
        github_enterprise_id: opt_string(&value, "githubEnterpriseId"),
        github_installation_id: opt_string(&value, "githubInstallationId"),
        active_mirror_transition_job_id: opt_string(&value, "activeMirrorTransitionJobId"),
        created_at: opt_string(&value, "createdAt"),
        updated_at: opt_string(&value, "updatedAt"),
        last_pushed_at: opt_string(&value, "lastPushedAt"),
        clone_url: opt_string(&value, "cloneUrl"),
    })
}

pub fn parse_namespaces(text: &str) -> Result<(Vec<Namespace>, String), String> {
    let value = serde_json::from_str::<Value>(text.trim())
        .map_err(|err| format!("bad namespaces: {err}"))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    let next = opt_string(&value, "nextPageToken").unwrap_or_default();
    let entries = value
        .get("namespaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut namespaces = Vec::new();
    for entry in entries {
        let ns = entry.get("namespace").unwrap_or(&entry);
        let Some(slug) = opt_string(ns, "slug") else {
            continue;
        };
        namespaces.push(Namespace {
            slug,
            kind: opt_string(ns, "type"),
            can_create: entry
                .get("viewerCanCreateRepositories")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        });
    }
    Ok((namespaces, next))
}

pub fn parse_repo_page(text: &str) -> Result<(Vec<ApiRepo>, String), String> {
    let value = serde_json::from_str::<Value>(text.trim())
        .map_err(|err| format!("bad repo list: {err}"))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    let next = opt_string(&value, "nextPageToken").unwrap_or_default();
    let entries = value
        .get("repositories")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut repos = Vec::new();
    for entry in entries {
        if let Some(repo) = api_repo_value(&entry) {
            repos.push(repo);
        }
    }
    Ok((repos, next))
}

pub fn parse_branches(text: &str) -> Result<(Vec<Branch>, String), String> {
    let value = serde_json::from_str::<Value>(text.trim())
        .map_err(|err| format!("bad branch list: {err}"))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    let next = opt_string(&value, "nextPageToken").unwrap_or_default();
    let entries = value
        .get("branches")
        .and_then(|entry| entry.as_array())
        .cloned()
        .unwrap_or_default();
    let mut branches = Vec::new();
    for entry in entries {
        let Some(name) = opt_string(&entry, "name") else {
            continue;
        };
        let sha = entry
            .get("commit")
            .and_then(|commit| opt_string(commit, "sha"))
            .unwrap_or_default();
        branches.push(Branch { name, sha });
    }
    Ok((branches, next))
}

pub fn parse_commits(text: &str) -> Result<Vec<Commit>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value = serde_json::from_str::<Value>(trimmed).map_err(|_| command_error(trimmed, ""))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    let entries = value
        .get("commits")
        .and_then(|entry| entry.as_array())
        .cloned()
        .unwrap_or_default();
    let mut commits = Vec::new();
    for entry in entries {
        if let Some(commit) = commit_value(&entry) {
            commits.push(commit);
        }
    }
    Ok(commits)
}

fn commit_value(value: &Value) -> Option<Commit> {
    let sha = opt_string(value, "sha")?;
    let commit = value.get("commit")?;
    let message = opt_string(commit, "message").unwrap_or_default();
    let summary = message.lines().next().unwrap_or("").trim().to_string();
    let author = commit
        .get("author")
        .and_then(|author| opt_string(author, "name"))
        .unwrap_or_default();
    let date = commit
        .get("author")
        .and_then(|author| opt_string(author, "date"))
        .unwrap_or_default();
    Some(Commit {
        sha,
        summary,
        author,
        date,
        url: opt_string(value, "webUrl").unwrap_or_default(),
    })
}

pub fn parse_pulls(text: &str) -> Result<Vec<PullRequest>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value = serde_json::from_str::<Value>(trimmed).map_err(|_| command_error(trimmed, ""))?;
    let Value::Array(items) = value else {
        if let Some(message) = api_message(&value) {
            return Err(message);
        }
        return Err("pull request list was not an array".to_string());
    };
    let mut pulls = Vec::new();
    for item in items {
        if let Some(pull) = pull_value(&item) {
            pulls.push(pull);
        }
    }
    pulls.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then(right.number.cmp(&left.number))
    });
    Ok(pulls)
}

fn pull_value(value: &Value) -> Option<PullRequest> {
    let number = match value.get("number")? {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|n| u64::try_from(n).ok())),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }?;
    Some(PullRequest {
        number,
        title: opt_string(value, "title").unwrap_or_default(),
        status: opt_string(value, "status").unwrap_or_else(|| "unknown".to_string()),
        url: opt_string(value, "url").unwrap_or_default(),
    })
}

fn status_rank(status: &str) -> u8 {
    match status.to_ascii_lowercase().as_str() {
        "open" => 0,
        "draft" => 1,
        "closed" => 2,
        "merged" => 3,
        _ => 4,
    }
}

pub fn parse_api_repo(text: &str) -> Result<ApiRepo, String> {
    let value =
        serde_json::from_str::<Value>(text.trim()).map_err(|err| format!("bad repo: {err}"))?;
    if let Some(message) = api_message(&value) {
        return Err(message);
    }
    api_repo_value(&value).ok_or_else(|| "repo response had no name".to_string())
}

fn api_repo_value(value: &Value) -> Option<ApiRepo> {
    let name = opt_string(value, "name")?;
    let full_name = opt_string(value, "fullName").unwrap_or(name);
    let owner = value.get("owner");
    let mirror = value.get("mirror");
    Some(ApiRepo {
        id: opt_string(value, "id"),
        full_name,
        default_branch: opt_string(value, "defaultBranch"),
        created_at: opt_string(value, "createdAt"),
        updated_at: opt_string(value, "updatedAt"),
        pushed_at: opt_string(value, "pushedAt"),
        owner_slug: owner.and_then(|owner| opt_string(owner, "slug")),
        owner_kind: owner.and_then(|owner| opt_string(owner, "type")),
        owner_id: owner.and_then(|owner| opt_string(owner, "id")),
        clone_url: opt_string(value, "cloneUrl"),
        web_url: opt_string(value, "webUrl"),
        visibility: opt_string(value, "visibility"),
        mirror_source: mirror.and_then(|mirror| opt_string(mirror, "source")),
        mirror_status: mirror.and_then(|mirror| opt_string(mirror, "status")),
        allow_merge_commit: opt_bool(value, "allowMergeCommit"),
        allow_squash_merge: opt_bool(value, "allowSquashMerge"),
        delete_branch_on_merge: opt_bool(value, "deleteBranchOnMerge"),
    })
}

fn api_message(value: &Value) -> Option<String> {
    if value.get("fullName").is_some()
        || value.get("repositories").is_some()
        || value.get("namespaces").is_some()
        || value.get("org").is_some()
    {
        return None;
    }
    opt_string(value, "message")
}

fn opt_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        other => Some(other.to_string()),
    }
}

fn opt_bool(value: &Value, key: &str) -> Option<bool> {
    match value.get(key)? {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) if text == "true" => Some(true),
        Value::String(text) if text == "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auth_and_repo_list() {
        let auth = parse_auth_status(
            "Account:     splaice@gmail.com\nAuth method: login\nEndpoint:    https://api2.cursor.sh\nToken:       valid\n",
        )
        .unwrap();
        assert_eq!(auth.account.as_deref(), Some("splaice@gmail.com"));
        assert!(auth.signed_in());

        let repos = parse_repo_list(
            "splaice/dotfiles\n  https://origin.cursor.com/splaice/dotfiles.git\nsplaice/garage\n  https://origin.cursor.com/splaice/garage.git\n",
        );
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "dotfiles");
        assert!(repos[1].clone_url.ends_with("garage.git"));
    }

    #[test]
    fn parses_view_and_api_repo() {
        let view = parse_view(
            r#"{"org":"splaice","name":"garage","defaultBranch":"main","repoUuid":"r_1","mirrorStatus":"no-mirror","forgeEnvironment":"prod","originNamespaceId":"ns_1","githubNodeId":null,"createdAt":"2026-09-06T03:41:15Z","updatedAt":"2026-09-15T20:28:33Z","lastPushedAt":"2026-09-15T20:28:33Z","cloneUrl":"https://origin.cursor.com/splaice/garage.git"}"#,
        )
        .unwrap();
        assert_eq!(view.mirror_status.as_deref(), Some("no-mirror"));
        assert!(view.github_node_id.is_none());

        let api = parse_api_repo(
            r#"{"id":"r_1","name":"dotfiles","fullName":"splaice/dotfiles","defaultBranch":"main","owner":{"slug":"splaice","id":"ns_1","type":"user"},"cloneUrl":"https://origin.cursor.com/splaice/dotfiles.git","mirror":{"source":"github","status":"inbound"},"visibility":"private","allowMergeCommit":true,"allowSquashMerge":true,"deleteBranchOnMerge":false,"webUrl":"https://cursor.com/codebase/splaice/dotfiles"}"#,
        )
        .unwrap();
        assert_eq!(api.mirror_source.as_deref(), Some("github"));
        assert_eq!(api.visibility.as_deref(), Some("private"));
        assert_eq!(api.allow_squash_merge, Some(true));
    }

    #[test]
    fn parse_pulls_sorts_open_before_closed() {
        let pulls = parse_pulls(
            r#"[
                {"number":"2","title":"Old fix","status":"closed","url":"https://example.com/2","author":"ada"},
                {"number":20,"title":"Landed","status":"merged","url":"https://example.com/20","author":{"displayName":"bea"}},
                {"number":3,"title":"Sketch","status":"draft","url":"https://example.com/3","author":{"id":"1","kind":"user","displayName":"cy"}},
                {"number":9,"title":"Add gate","status":"open","url":"https://example.com/9","headRef":"gate","baseRef":"main","updatedAt":"2026-10-01T00:00:00Z"}
            ]"#,
        )
        .unwrap();
        let statuses: Vec<_> = pulls.iter().map(|pull| pull.status.as_str()).collect();
        assert_eq!(statuses, ["open", "draft", "closed", "merged"]);
        assert_eq!(pulls[0].number, 9);
        assert_eq!(pulls[2].number, 2);
        assert!(parse_pulls(r#"{"message":"nope"}"#).is_err());
    }

    #[test]
    fn parses_branches_and_commits() {
        let (branches, next) = parse_branches(
            r#"{"branches":[{"name":"main","commit":{"sha":"aaa"}},{"name":"topic/one","commit":{"sha":"bbb"}}],"nextPageToken":""}"#,
        )
        .unwrap();
        assert!(next.is_empty());
        assert_eq!(branches[1].name, "topic/one");
        assert_eq!(branches[1].sha, "bbb");
        let path = commits_path("acme/widget", "topic/one").unwrap();
        assert!(path.contains("sha=topic%2Fone"), "{path}");
        let commits = parse_commits(
            r#"{"commits":[{"sha":"abcdef0123456789","commit":{"author":{"name":"Ada","email":"a@example.com","date":"2026-10-01T12:00:00Z"},"message":"Add gate\n\nbody"},"webUrl":"https://example.com/c"}],"nextPageToken":"x"}"#,
        )
        .unwrap();
        assert_eq!(commits[0].summary, "Add gate");
        assert_eq!(commits[0].author, "Ada");
        assert_eq!(commits[0].sha, "abcdef0123456789");
        assert!(parse_commits(r#"{"message":"empty repository"}"#).is_err());
    }

    #[test]
    fn update_args_uses_raw_visibility_and_typed_merge_flags() {
        let visibility =
            update_args("acme/widget", &RepoPatch::Visibility("private".to_string())).unwrap();
        assert!(
            visibility
                .windows(2)
                .any(|pair| { pair[0] == "-f" && pair[1] == "visibility=private" })
        );
        assert!(visibility.iter().any(|arg| arg == "-X"));
        assert!(visibility.iter().any(|arg| arg == "PATCH"));

        let merge = update_args(
            "acme/widget",
            &RepoPatch::Merge {
                allow_merge: true,
                allow_squash: false,
            },
        )
        .unwrap();
        assert!(
            merge
                .windows(2)
                .any(|pair| { pair[0] == "-F" && pair[1] == "allowMergeCommit=true" })
        );
        assert!(
            merge
                .windows(2)
                .any(|pair| { pair[0] == "-F" && pair[1] == "allowSquashMerge=false" })
        );
        assert!(
            update_args(
                "acme/widget",
                &RepoPatch::Merge {
                    allow_merge: false,
                    allow_squash: false,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn create_command_includes_the_namespace() {
        assert_eq!(
            create_args("splaice", "widget", "main"),
            vec![
                "repo".to_string(),
                "create".to_string(),
                "splaice/widget".to_string(),
                "--default-branch".to_string(),
                "main".to_string(),
            ]
        );
    }
}
