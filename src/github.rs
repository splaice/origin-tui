use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const GH_TIMEOUT: Duration = Duration::from_secs(45);
const GIT_TIMEOUT: Duration = Duration::from_secs(600);

pub const SYNC_NOTE: &str = "Origin is the source. Sync makes GitHub match it, including removing branches and tags that exist only on GitHub. Visibility is used only when the GitHub repository is created.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Child {
    pub github: String,
    pub visibility: String,
}

pub fn web_url(github: &str) -> String {
    format!("https://github.com/{github}")
}

pub fn ssh_url(github: &str) -> String {
    format!("git@github.com:{github}.git")
}

pub fn view_args(github: &str) -> Vec<String> {
    vec![
        "repo".to_string(),
        "view".to_string(),
        github.to_string(),
        "--json".to_string(),
        "name".to_string(),
    ]
}

pub fn visibility_flag(visibility: &str) -> &'static str {
    match visibility {
        "public" => "--public",
        "internal" => "--internal",
        _ => "--private",
    }
}

pub fn create_args(github: &str, visibility: &str) -> Vec<String> {
    vec![
        "repo".to_string(),
        "create".to_string(),
        github.to_string(),
        visibility_flag(visibility).to_string(),
    ]
}

pub fn clone_args(clone_url: &str, dest: &str) -> Vec<String> {
    vec![
        "clone".to_string(),
        "--bare".to_string(),
        clone_url.to_string(),
        dest.to_string(),
    ]
}

pub fn push_args(repo_dir: &str, github: &str) -> Vec<String> {
    vec![
        "-C".to_string(),
        repo_dir.to_string(),
        "push".to_string(),
        "--mirror".to_string(),
        ssh_url(github),
    ]
}

pub fn sync_preview(github: &str, visibility: &str) -> String {
    let github = github.trim();
    if github.is_empty() {
        return "git push --mirror git@github.com:<owner/repo>.git".to_string();
    }
    format!(
        "gh repo create {github} {}  ·  git push --mirror {}",
        visibility_flag(visibility),
        ssh_url(github)
    )
}

pub fn children_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".origin-tui")
            .join("children.json"),
    )
}

pub fn load_children(path: &Path) -> Result<BTreeMap<String, Child>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_children(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(err) => Err(format!("Could not read GitHub children: {err}")),
    }
}

pub fn parse_children(text: &str) -> Result<BTreeMap<String, Child>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| format!("GitHub children file is not valid JSON: {err}"))?;
    let Some(obj) = value.as_object() else {
        return Err("GitHub children file is not an object.".to_string());
    };
    let mut map = BTreeMap::new();
    for (origin, entry) in obj {
        if origin.is_empty() {
            continue;
        }
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(github) = entry.get("github").and_then(Value::as_str) else {
            continue;
        };
        if github.is_empty() || !safe_github(github) {
            continue;
        }
        let visibility = entry
            .get("visibility")
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "private" | "public" | "internal"))
            .unwrap_or("private");
        map.insert(
            origin.clone(),
            Child {
                github: github.to_string(),
                visibility: visibility.to_string(),
            },
        );
    }
    Ok(map)
}

pub fn save_children(path: &Path, children: &BTreeMap<String, Child>) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Could not save GitHub children: {err}"))?;
    }
    let mut root = serde_json::Map::new();
    for (origin, child) in children {
        let mut entry = serde_json::Map::new();
        entry.insert("github".to_string(), Value::String(child.github.clone()));
        entry.insert(
            "visibility".to_string(),
            Value::String(child.visibility.clone()),
        );
        root.insert(origin.clone(), Value::Object(entry));
    }
    let text = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|err| format!("Could not save GitHub children: {err}"))?;
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    std::fs::write(&tmp, text).map_err(|err| format!("Could not save GitHub children: {err}"))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("Could not save GitHub children: {err}"))
}

pub fn repository_missing(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("could not resolve to a repository")
        || lower.contains("no repository found")
        || (lower.contains("404") && lower.contains("not found"))
}

pub fn repository_exists(message: &str) -> bool {
    message.to_ascii_lowercase().contains("already exists")
}

pub fn sync(clone_url: &str, github: &str, visibility: &str) -> Result<(), String> {
    if !safe_clone_url(clone_url) {
        return Err("Refusing an unsafe clone URL.".to_string());
    }
    if !safe_github(github) {
        return Err("Use a GitHub repository as owner/repo.".to_string());
    }
    let gh = which("gh")
        .ok_or_else(|| "gh CLI not found. Install GitHub CLI and run gh auth login.".to_string())?;
    let git = which("git").ok_or_else(|| "git not found".to_string())?;
    ensure_repo(&gh, github, visibility)?;
    let dir = bare_dir();
    let guard = BareDir(dir.clone());
    let dest = dir.display().to_string();
    run(&git, &clone_args(clone_url, &dest), GIT_TIMEOUT, true)?;
    run(&git, &push_args(&dest, github), GIT_TIMEOUT, true)?;
    drop(guard);
    Ok(())
}

fn ensure_repo(gh: &Path, github: &str, visibility: &str) -> Result<(), String> {
    match run(gh, &view_args(github), GH_TIMEOUT, false) {
        Ok(_) => Ok(()),
        Err(err) if repository_missing(&err) => {
            match run(gh, &create_args(github, visibility), GH_TIMEOUT, false) {
                Ok(_) => Ok(()),
                Err(err) if repository_exists(&err) => Ok(()),
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn safe_github(github: &str) -> bool {
    let Some((owner, name)) = github.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !name.is_empty()
        && !name.contains('/')
        && !owner.starts_with('-')
        && !name.starts_with('-')
        && owner.chars().all(safe_token)
        && name.chars().all(safe_token)
}

fn safe_token(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

fn safe_clone_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("ssh://") || url.starts_with("git://"))
        && !url.chars().any(|ch| ch.is_whitespace() || ch.is_control())
}

fn bare_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("ot-sync-{}-{nanos}", std::process::id()))
}

struct BareDir(PathBuf);

impl Drop for BareDir {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
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

fn run(program: &Path, args: &[String], timeout: Duration, git: bool) -> Result<String, String> {
    let name = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("command");
    let mut command = Command::new(program);
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if git {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to run {name}: {err}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{name} returned no stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{name} returned no stderr"))?;
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
            Ok(None) if started.elapsed() >= timeout => {
                terminate(&mut child);
                return Err(format!("{name} timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(format!("{name} failed: {err}")),
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        Err(tail_error(name, &stdout, &stderr))
    }
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn tail_error(program: &str, stdout: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    let raw = if !stderr.is_empty() { stderr } else { stdout };
    if raw.is_empty() {
        return format!("{program} failed");
    }
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(6);
    let mut text = redact(&lines[start..].join(" "));
    if text.chars().count() > 320 {
        text = text.chars().take(320).collect();
        text.push('…');
    }
    format!("{program}: {text}")
}

fn redact(text: &str) -> String {
    const PREFIXES: [&str; 6] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let found = PREFIXES
            .iter()
            .filter_map(|prefix| rest.find(prefix).map(|index| (index, *prefix)));
        let Some((index, prefix)) = found.min_by_key(|(index, _)| *index) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        let after = &rest[index + prefix.len()..];
        let body = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .count();
        if body == 0 {
            out.push_str(prefix);
            rest = after;
            continue;
        }
        let body_len: usize = after.chars().take(body).map(char::len_utf8).sum();
        out.push_str("[redacted]");
        rest = &after[body_len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_commands_mirror_origin_onto_github() {
        assert_eq!(web_url("acme/widget"), "https://github.com/acme/widget");
        assert_eq!(ssh_url("acme/widget"), "git@github.com:acme/widget.git");
        assert_eq!(
            view_args("acme/widget"),
            [
                "repo".to_string(),
                "view".to_string(),
                "acme/widget".to_string(),
                "--json".to_string(),
                "name".to_string(),
            ]
        );
        let created = create_args("acme/widget", "private");
        assert_eq!(
            created,
            [
                "repo".to_string(),
                "create".to_string(),
                "acme/widget".to_string(),
                "--private".to_string(),
            ]
        );
        assert!(!created.iter().any(|arg| arg.contains("readme")));
        assert_eq!(create_args("acme/widget", "public")[3], "--public");
        assert_eq!(create_args("acme/widget", "internal")[3], "--internal");
        assert_eq!(create_args("acme/widget", "nope")[3], "--private");
        assert_eq!(
            clone_args("https://origin.cursor.com/acme/widget.git", "/tmp/ot-bare"),
            [
                "clone".to_string(),
                "--bare".to_string(),
                "https://origin.cursor.com/acme/widget.git".to_string(),
                "/tmp/ot-bare".to_string(),
            ]
        );
        assert_eq!(
            push_args("/tmp/ot-bare", "acme/widget"),
            [
                "-C".to_string(),
                "/tmp/ot-bare".to_string(),
                "push".to_string(),
                "--mirror".to_string(),
                "git@github.com:acme/widget.git".to_string(),
            ]
        );
        let preview = sync_preview("acme/widget", "private");
        assert!(preview.contains("git push --mirror"), "{preview}");
        assert!(preview.contains("--private"), "{preview}");
        assert!(!preview.contains("readme"), "{preview}");
    }

    #[test]
    fn missing_and_existing_repositories_are_distinct() {
        assert!(repository_missing(
            "GraphQL: Could not resolve to a Repository with the name 'acme/widget'. (repository)"
        ));
        assert!(repository_missing(
            "HTTP 404: Not Found (https://api.github.com/repos/acme/widget)"
        ));
        assert!(!repository_missing("HTTP 401: Bad credentials"));
        assert!(!repository_missing(
            "gh: To use GitHub CLI in automation, set the GH_TOKEN environment variable."
        ));
        assert!(repository_exists(
            "HTTP 422: Repository creation failed. Name already exists on this account"
        ));
        assert!(!repository_exists("HTTP 401: Bad credentials"));
    }

    #[test]
    fn children_roundtrip_through_an_explicit_path() {
        let path =
            std::env::temp_dir().join(format!("ot-children-{}-roundtrip.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(load_children(&path).unwrap().is_empty());
        let mut map = BTreeMap::new();
        map.insert(
            "acme/widget".to_string(),
            Child {
                github: "acme/checkout".to_string(),
                visibility: "public".to_string(),
            },
        );
        save_children(&path, &map).unwrap();
        assert_eq!(load_children(&path).unwrap(), map);
        let _ = std::fs::remove_file(&path);
        assert!(parse_children("[]").is_err());
        let parsed = parse_children(
            r#"{"acme/widget":{"github":"acme/checkout","visibility":"secret","token":"ghp_secret"},"skip":{"visibility":"private"}}"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["acme/widget"].visibility, "private");
        assert!(!parsed["acme/widget"].github.contains("ghp_"));
    }

    #[test]
    fn errors_drop_credential_shaped_tokens() {
        let text = redact("fatal: auth ghp_abcdefghijklmnopqrstuvwxyz failed");
        assert!(!text.contains("ghp_abc"));
        assert!(text.contains("[redacted]"));
    }
}
