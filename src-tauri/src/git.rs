use crate::workspace::{is_sensitive_path, workspace_root, WorkspaceState};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Component, Path},
    process::Command as StdCommand,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tokio::process::Command;

const MAX_OUTPUT_CHARS: usize = 40_000;
const MAX_PROPOSALS: usize = 100;
const MAX_PATHS_PER_ACTION: usize = 200;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    path: String,
    index_status: String,
    worktree_status: String,
    staged: bool,
    unstaged: bool,
    untracked: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    name: String,
    current: bool,
    upstream: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    sha: String,
    subject: String,
    author: String,
    relative_time: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    name: String,
    fetch_url: String,
    push_url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubStatus {
    gh_installed: bool,
    authenticated: bool,
    repository: Option<String>,
    web_url: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    is_repository: bool,
    branch: Option<String>,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
    clean: bool,
    files: Vec<GitFileStatus>,
    branches: Vec<GitBranch>,
    commits: Vec<GitCommit>,
    remotes: Vec<GitRemote>,
    github: GitHubStatus,
}

#[derive(Clone)]
enum GitAction {
    Stage { paths: Vec<String> },
    Unstage { paths: Vec<String> },
    Commit { message: String },
    CreateBranch { branch: String },
    SwitchBranch { branch: String },
    Pull,
    Push,
    PushSetUpstream { remote: String, branch: String },
    CreatePullRequest { title: String, body: String, draft: bool },
}

impl GitAction {
    fn kind(&self) -> &'static str {
        match self {
            Self::Stage { .. } => "stage",
            Self::Unstage { .. } => "unstage",
            Self::Commit { .. } => "commit",
            Self::CreateBranch { .. } => "create-branch",
            Self::SwitchBranch { .. } => "switch-branch",
            Self::Pull => "pull",
            Self::Push => "push",
            Self::PushSetUpstream { .. } => "push-set-upstream",
            Self::CreatePullRequest { .. } => "create-pull-request",
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Stage { paths } => format!("Stage {} file(s)", paths.len()),
            Self::Unstage { paths } => format!("Unstage {} file(s)", paths.len()),
            Self::Commit { message } => format!("Commit: {}", message.lines().next().unwrap_or("")),
            Self::CreateBranch { branch } => format!("Create and switch to branch {branch}"),
            Self::SwitchBranch { branch } => format!("Switch to branch {branch}"),
            Self::Pull => "Pull from upstream using fast-forward only".to_string(),
            Self::Push => "Push current branch to its configured upstream".to_string(),
            Self::PushSetUpstream { remote, branch } => {
                format!("Push {branch} and set upstream to {remote}/{branch}")
            }
            Self::CreatePullRequest { title, draft, .. } => {
                format!("Create {}pull request: {title}", if *draft { "draft " } else { "" })
            }
        }
    }
}

#[derive(Clone)]
struct GitActionProposalInternal {
    id: String,
    action: GitAction,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActionProposal {
    id: String,
    kind: String,
    summary: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActionResult {
    kind: String,
    success: bool,
    output: String,
    web_url: Option<String>,
}

#[derive(Default)]
pub struct GitState {
    pending: Mutex<HashMap<String, GitActionProposalInternal>>,
    sequence: AtomicU64,
}

fn next_id(state: &GitState) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    format!("git-{millis}-{sequence}")
}

fn trim_output(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .chars()
        .take(MAX_OUTPUT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn ensure_git_repository(root: &Path) -> Result<(), String> {
    let output = StdCommand::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("Unable to run Git: {error}"))?;

    if output.status.success() && trim_output(&output.stdout) == "true" {
        Ok(())
    } else {
        Err("The current workspace is not a Git repository.".to_string())
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("Unable to run Git: {error}"))?;

    if output.status.success() {
        Ok(trim_output(&output.stdout))
    } else {
        let error = trim_output(&output.stderr);
        Err(if error.is_empty() {
            format!("Git command failed: git {}", args.join(" "))
        } else {
            error
        })
    }
}

fn validate_relative_path(path: &str) -> Result<String, String> {
    let value = path.trim();
    let candidate = Path::new(value);

    if value.is_empty()
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
        || candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Git file paths must stay inside the current workspace.".to_string());
    }

    Ok(value.replace('\\', "/"))
}

fn validate_paths(paths: Vec<String>, block_sensitive: bool) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("Select at least one file.".to_string());
    }

    if paths.len() > MAX_PATHS_PER_ACTION {
        return Err("Too many files selected for one Git action.".to_string());
    }

    paths
        .into_iter()
        .map(|path| {
            let normalized = validate_relative_path(&path)?;
            if block_sensitive && is_sensitive_path(Path::new(&normalized)) {
                return Err(format!(
                    "BOSCode will not stage sensitive credential material: {normalized}"
                ));
            }
            Ok(normalized)
        })
        .collect()
}

fn staged_sensitive_paths(root: &Path) -> Vec<String> {
    let Ok(output) = git_output(root, &["diff", "--cached", "--name-only", "--diff-filter=ACMR"]) else {
        return Vec::new();
    };

    output
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty() && is_sensitive_path(Path::new(path)))
        .map(ToOwned::to_owned)
        .collect()
}

fn validate_commit_message(message: String) -> Result<String, String> {
    let message = message.trim().to_string();

    if message.is_empty() {
        return Err("Commit message cannot be empty.".to_string());
    }

    if message.len() > 4_000 {
        return Err("Commit message is too long.".to_string());
    }

    Ok(message)
}

fn validate_branch(root: &Path, branch: String) -> Result<String, String> {
    let branch = branch.trim().to_string();

    if branch.is_empty() || branch.len() > 240 || branch.contains('\n') || branch.contains('\r') {
        return Err("Branch name is invalid.".to_string());
    }

    let output = StdCommand::new("git")
        .args(["check-ref-format", "--branch", &branch])
        .current_dir(root)
        .output()
        .map_err(|error| format!("Unable to validate branch name: {error}"))?;

    if output.status.success() {
        Ok(branch)
    } else {
        Err("Git rejected this branch name.".to_string())
    }
}

fn validate_remote(remote: String) -> Result<String, String> {
    let remote = remote.trim().to_string();
    let valid = !remote.is_empty()
        && remote.len() <= 100
        && remote
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'));

    if valid {
        Ok(remote)
    } else {
        Err("Remote name is invalid.".to_string())
    }
}

fn read_branch(root: &Path) -> Option<String> {
    git_output(root, &["branch", "--show-current"])
        .ok()
        .filter(|value| !value.is_empty())
}

fn read_upstream(root: &Path) -> Option<String> {
    git_output(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
        .ok()
        .filter(|value| !value.is_empty())
}

fn ahead_behind(root: &Path) -> (usize, usize) {
    let Ok(value) = git_output(root, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) else {
        return (0, 0);
    };

    let mut parts = value.split_whitespace();
    let ahead = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    let behind = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    (ahead, behind)
}

fn parse_status(root: &Path) -> Vec<GitFileStatus> {
    let Ok(output) = git_output(root, &["status", "--porcelain=v1", "--untracked-files=normal"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            if line.len() < 3 {
                return None;
            }

            let bytes = line.as_bytes();
            let index = bytes[0] as char;
            let worktree = bytes[1] as char;
            let mut path = line[3..].to_string();

            if let Some((_, renamed_to)) = path.rsplit_once(" -> ") {
                path = renamed_to.to_string();
            }

            let untracked = index == '?' && worktree == '?';
            Some(GitFileStatus {
                path,
                index_status: index.to_string(),
                worktree_status: worktree.to_string(),
                staged: !untracked && index != ' ',
                unstaged: untracked || worktree != ' ',
                untracked,
            })
        })
        .collect()
}

fn parse_branches(root: &Path) -> Vec<GitBranch> {
    let format = "%(HEAD)%09%(refname:short)%09%(upstream:short)";
    let Ok(output) = git_output(root, &["for-each-ref", "--sort=-committerdate", &format!("--format={format}"), "refs/heads"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let head = parts.next()?;
            let name = parts.next()?.to_string();
            let upstream = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);

            Some(GitBranch {
                name,
                current: head.trim() == "*",
                upstream,
            })
        })
        .collect()
}

fn parse_commits(root: &Path) -> Vec<GitCommit> {
    let format = "%h%x09%s%x09%an%x09%ar";
    let Ok(output) = git_output(root, &["log", "-15", &format!("--pretty=format:{format}")]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            Some(GitCommit {
                sha: parts.next()?.to_string(),
                subject: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                relative_time: parts.next()?.to_string(),
            })
        })
        .collect()
}

fn parse_remotes(root: &Path) -> Vec<GitRemote> {
    let Ok(names) = git_output(root, &["remote"]) else {
        return Vec::new();
    };

    names
        .lines()
        .filter_map(|name| {
            let fetch_url = git_output(root, &["remote", "get-url", name]).ok()?;
            let push_url = git_output(root, &["remote", "get-url", "--push", name])
                .unwrap_or_else(|_| fetch_url.clone());
            Some(GitRemote {
                name: name.to_string(),
                fetch_url,
                push_url,
            })
        })
        .collect()
}

fn github_repo_from_remote(remote: &str) -> Option<(String, String)> {
    let trimmed = remote.trim().trim_end_matches(".git");

    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        let repo = rest.trim_matches('/').to_string();
        if repo.split('/').count() == 2 {
            return Some((repo.clone(), format!("https://github.com/{repo}")));
        }
    }

    for prefix in ["https://github.com/", "http://github.com/", "ssh://git@github.com/"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let repo = rest.trim_matches('/').to_string();
            if repo.split('/').count() == 2 {
                return Some((repo.clone(), format!("https://github.com/{repo}")));
            }
        }
    }

    None
}

fn github_status(remotes: &[GitRemote]) -> GitHubStatus {
    let repository = remotes
        .iter()
        .find_map(|remote| github_repo_from_remote(&remote.fetch_url));

    let gh_installed = which::which("gh").is_ok();
    let authenticated = gh_installed
        && StdCommand::new("gh")
            .args(["auth", "status", "--hostname", "github.com"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);

    GitHubStatus {
        gh_installed,
        authenticated,
        repository: repository.as_ref().map(|(repo, _)| repo.clone()),
        web_url: repository.map(|(_, url)| url),
    }
}

fn view_proposal(proposal: &GitActionProposalInternal) -> GitActionProposal {
    GitActionProposal {
        id: proposal.id.clone(),
        kind: proposal.action.kind().to_string(),
        summary: proposal.action.summary(),
    }
}

async fn run_git(root: &Path, args: &[String]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| format!("Unable to run Git: {error}"))?;

    if output.status.success() {
        let stdout = trim_output(&output.stdout);
        let stderr = trim_output(&output.stderr);
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        let error = trim_output(&output.stderr);
        Err(if error.is_empty() {
            format!("Git command failed: git {}", args.join(" "))
        } else {
            error
        })
    }
}

async fn run_gh(root: &Path, args: &[String]) -> Result<String, String> {
    if which::which("gh").is_err() {
        return Err("GitHub CLI (gh) is not installed or not available on PATH.".to_string());
    }

    let output = Command::new("gh")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| format!("Unable to run GitHub CLI: {error}"))?;

    if output.status.success() {
        Ok(trim_output(&output.stdout))
    } else {
        let error = trim_output(&output.stderr);
        Err(if error.is_empty() {
            "GitHub CLI command failed.".to_string()
        } else {
            error
        })
    }
}

#[tauri::command]
pub fn git_snapshot(workspace: State<'_, WorkspaceState>) -> Result<GitSnapshot, String> {
    let root = workspace_root(&workspace)?;
    if ensure_git_repository(&root).is_err() {
        return Ok(GitSnapshot {
            is_repository: false,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            clean: true,
            files: Vec::new(),
            branches: Vec::new(),
            commits: Vec::new(),
            remotes: Vec::new(),
            github: GitHubStatus {
                gh_installed: which::which("gh").is_ok(),
                authenticated: false,
                repository: None,
                web_url: None,
            },
        });
    }

    let files = parse_status(&root);
    let branches = parse_branches(&root);
    let commits = parse_commits(&root);
    let remotes = parse_remotes(&root);
    let (ahead, behind) = ahead_behind(&root);

    Ok(GitSnapshot {
        is_repository: true,
        branch: read_branch(&root),
        upstream: read_upstream(&root),
        ahead,
        behind,
        clean: files.is_empty(),
        files,
        branches,
        commits,
        github: github_status(&remotes),
        remotes,
    })
}

#[tauri::command]
pub fn list_git_action_proposals(
    state: State<'_, GitState>,
) -> Result<Vec<GitActionProposal>, String> {
    let pending = state
        .pending
        .lock()
        .map_err(|_| "Git approval state is unavailable.".to_string())?;

    let mut items = pending.values().map(view_proposal).collect::<Vec<_>>();
    items.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(items)
}

fn store_proposal(
    state: &State<'_, GitState>,
    action: GitAction,
) -> Result<GitActionProposal, String> {
    let internal = GitActionProposalInternal {
        id: next_id(state),
        action,
    };
    let view = view_proposal(&internal);

    let mut pending = state
        .pending
        .lock()
        .map_err(|_| "Git approval state is unavailable.".to_string())?;

    if pending.len() >= MAX_PROPOSALS {
        return Err("Too many pending Git actions. Execute or reject some first.".to_string());
    }

    pending.insert(internal.id.clone(), internal);
    Ok(view)
}

#[tauri::command]
pub fn propose_git_stage(
    paths: Vec<String>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    store_proposal(&state, GitAction::Stage { paths: validate_paths(paths, true)? })
}

#[tauri::command]
pub fn propose_git_unstage(
    paths: Vec<String>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    store_proposal(&state, GitAction::Unstage { paths: validate_paths(paths, false)? })
}

#[tauri::command]
pub fn propose_git_commit(
    message: String,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    let sensitive = staged_sensitive_paths(&root);
    if !sensitive.is_empty() {
        return Err(format!(
            "Commit blocked because sensitive files are staged: {}",
            sensitive.join(", ")
        ));
    }

    store_proposal(
        &state,
        GitAction::Commit {
            message: validate_commit_message(message)?,
        },
    )
}

#[tauri::command]
pub fn propose_git_create_branch(
    branch: String,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    let branch = validate_branch(&root, branch)?;
    store_proposal(&state, GitAction::CreateBranch { branch })
}

#[tauri::command]
pub fn propose_git_switch_branch(
    branch: String,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    let branch = validate_branch(&root, branch)?;
    store_proposal(&state, GitAction::SwitchBranch { branch })
}

#[tauri::command]
pub fn propose_git_pull(
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;
    if read_upstream(&root).is_none() {
        return Err("The current branch has no upstream. Configure one before pulling.".to_string());
    }
    store_proposal(&state, GitAction::Pull)
}

#[tauri::command]
pub fn propose_git_push(
    set_upstream: Option<bool>,
    remote: Option<String>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;

    if read_upstream(&root).is_some() && !set_upstream.unwrap_or(false) {
        return store_proposal(&state, GitAction::Push);
    }

    let branch = read_branch(&root)
        .ok_or_else(|| "BOSCode cannot push while Git is in detached HEAD state.".to_string())?;
    let remote = validate_remote(remote.unwrap_or_else(|| "origin".to_string()))?;

    store_proposal(
        &state,
        GitAction::PushSetUpstream { remote, branch },
    )
}

#[tauri::command]
pub fn propose_github_pull_request(
    title: String,
    body: String,
    draft: Option<bool>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionProposal, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;

    if which::which("gh").is_err() {
        return Err("Install GitHub CLI (gh) before creating pull requests from BOSCode.".to_string());
    }

    let title = title.trim().to_string();
    if title.is_empty() || title.len() > 250 {
        return Err("Pull request title must be between 1 and 250 characters.".to_string());
    }

    if body.len() > 20_000 {
        return Err("Pull request body is too long.".to_string());
    }

    store_proposal(
        &state,
        GitAction::CreatePullRequest {
            title,
            body,
            draft: draft.unwrap_or(false),
        },
    )
}

#[tauri::command]
pub fn reject_git_action(
    action_id: String,
    state: State<'_, GitState>,
) -> Result<bool, String> {
    Ok(state
        .pending
        .lock()
        .map_err(|_| "Git approval state is unavailable.".to_string())?
        .remove(&action_id)
        .is_some())
}

#[tauri::command]
pub async fn execute_git_action(
    action_id: String,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, GitState>,
) -> Result<GitActionResult, String> {
    let root = workspace_root(&workspace)?;
    ensure_git_repository(&root)?;

    let proposal = state
        .pending
        .lock()
        .map_err(|_| "Git approval state is unavailable.".to_string())?
        .remove(&action_id)
        .ok_or_else(|| "This Git action is not pending approval.".to_string())?;

    let kind = proposal.action.kind().to_string();

    let result = match proposal.action {
        GitAction::Stage { paths } => {
            let mut args = vec!["add".to_string(), "--".to_string()];
            args.extend(paths);
            run_git(&root, &args).await.map(|output| (output, None))
        }
        GitAction::Unstage { paths } => {
            let mut args = vec![
                "restore".to_string(),
                "--staged".to_string(),
                "--".to_string(),
            ];
            args.extend(paths);
            run_git(&root, &args).await.map(|output| (output, None))
        }
        GitAction::Commit { message } => {
            run_git(
                &root,
                &["commit".to_string(), "-m".to_string(), message],
            )
            .await
            .map(|output| (output, None))
        }
        GitAction::CreateBranch { branch } => {
            run_git(
                &root,
                &["switch".to_string(), "-c".to_string(), branch],
            )
            .await
            .map(|output| (output, None))
        }
        GitAction::SwitchBranch { branch } => {
            run_git(&root, &["switch".to_string(), branch])
                .await
                .map(|output| (output, None))
        }
        GitAction::Pull => {
            run_git(&root, &["pull".to_string(), "--ff-only".to_string()])
                .await
                .map(|output| (output, None))
        }
        GitAction::Push => run_git(&root, &["push".to_string()])
            .await
            .map(|output| (output, None)),
        GitAction::PushSetUpstream { remote, branch } => {
            run_git(
                &root,
                &[
                    "push".to_string(),
                    "--set-upstream".to_string(),
                    remote,
                    branch,
                ],
            )
            .await
            .map(|output| (output, None))
        }
        GitAction::CreatePullRequest { title, body, draft } => {
            let mut args = vec![
                "pr".to_string(),
                "create".to_string(),
                "--title".to_string(),
                title,
                "--body".to_string(),
                body,
            ];
            if draft {
                args.push("--draft".to_string());
            }

            run_gh(&root, &args).await.map(|output| {
                let web_url = output
                    .lines()
                    .find(|line| line.trim().starts_with("http://") || line.trim().starts_with("https://"))
                    .map(|line| line.trim().to_string());
                (output, web_url)
            })
        }
    };

    match result {
        Ok((output, web_url)) => Ok(GitActionResult {
            kind,
            success: true,
            output,
            web_url,
        }),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{github_repo_from_remote, validate_commit_message, validate_relative_path, validate_remote};

    #[test]
    fn parses_common_github_remote_formats() {
        assert_eq!(
            github_repo_from_remote("git@github.com:owner/repo.git").map(|value| value.0),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            github_repo_from_remote("https://github.com/owner/repo.git").map(|value| value.0),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn rejects_workspace_escape_paths() {
        assert!(validate_relative_path("../secret.txt").is_err());
        assert!(validate_relative_path("C:\\Windows\\system.ini").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn validates_remote_names() {
        assert_eq!(validate_remote("origin".into()).unwrap(), "origin");
        assert!(validate_remote("bad remote".into()).is_err());
    }

    #[test]
    fn commit_messages_are_non_empty_and_bounded() {
        assert!(validate_commit_message("".into()).is_err());
        assert_eq!(
            validate_commit_message("feat: add Git panel".into()).unwrap(),
            "feat: add Git panel"
        );
    }
}
