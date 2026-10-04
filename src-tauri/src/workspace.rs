mod index;

use index::WorkspaceIndex;
use serde::Serialize;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::State;
use walkdir::DirEntry;

const MAX_WORKSPACE_ENTRIES: usize = 6_000;
const MAX_READ_BYTES: u64 = 1_500_000;
const MAX_CONTEXT_CHARS: usize = 32_000;
const INDEX_REFRESH_SECONDS: u64 = 2;

#[derive(Default)]
pub struct WorkspaceState {
    root: Arc<Mutex<Option<PathBuf>>>,
    index: Arc<Mutex<Option<WorkspaceIndex>>>,
    refreshing: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    root: String,
    name: String,
    branch: Option<String>,
    file_count: usize,
    directory_count: usize,
    truncated: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    path: String,
    name: String,
    is_dir: bool,
    depth: usize,
    size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    path: String,
    content: String,
    size: u64,
    language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    path: String,
    line: usize,
    preview: String,
}

fn ignored_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return false;
    }

    matches!(
        entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str(),
        ".git"
            | "node_modules"
            | "target"
            | "vendor"
            | "dist"
            | "build"
            | ".next"
            | ".idea"
            | ".vscode"
            | "coverage"
            | ".turbo"
    )
}

pub(crate) fn is_sensitive_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    let name = name.to_ascii_lowercase();

    if name == ".env.example" || name == ".env.sample" {
        return false;
    }

    name == ".env"
        || name.starts_with(".env.")
        || matches!(
            name.as_str(),
            "id_rsa"
                | "id_dsa"
                | "id_ed25519"
                | "credentials"
                | "credentials.json"
                | "secrets.json"
                | "service-account.json"
                | "service_account.json"
        )
        || name.ends_with(".pem")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".key")
}

pub(crate) fn workspace_root(state: &State<'_, WorkspaceState>) -> Result<PathBuf, String> {
    state
        .root
        .lock()
        .map_err(|_| "Workspace state is unavailable.".to_string())?
        .clone()
        .ok_or_else(|| "Open a workspace folder first.".to_string())
}

fn relative_display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_workspace_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);

    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("The requested path is outside the workspace.".to_string());
    }

    let candidate = root.join(relative);
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "The requested file does not exist.".to_string())?;

    if !canonical.starts_with(root) {
        return Err("The requested path is outside the workspace.".to_string());
    }

    if is_sensitive_path(&canonical) {
        return Err("BOSCode blocks this sensitive file from repository context.".to_string());
    }

    Ok(canonical)
}

fn language_for_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" => "JavaScript",
        "php" => "PHP",
        "py" => "Python",
        "cs" => "C#",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "dart" => "Dart",
        "go" => "Go",
        "rb" => "Ruby",
        "html" => "HTML",
        "css" | "scss" | "sass" => "CSS",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "md" => "Markdown",
        "sql" => "SQL",
        "xml" => "XML",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        _ => "Text",
    }
    .to_string()
}

fn read_git_branch(root: &Path) -> Option<String> {
    let head = fs::read_to_string(root.join(".git").join("HEAD")).ok()?;
    let head = head.trim();

    if let Some(reference) = head.strip_prefix("ref: refs/heads/") {
        return Some(reference.to_string());
    }

    if head.len() >= 7 {
        return Some(head.chars().take(7).collect());
    }

    None
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn workspace_summary_from_index(root: &Path, index: &WorkspaceIndex) -> WorkspaceSummary {
    WorkspaceSummary {
        root: root.to_string_lossy().to_string(),
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace")
            .to_string(),
        branch: read_git_branch(root),
        file_count: index.file_count,
        directory_count: index.directory_count,
        truncated: index.truncated,
    }
}

fn refresh_workspace_index(
    state: &State<'_, WorkspaceState>,
    force: bool,
) -> Result<PathBuf, String> {
    let root = workspace_root(state)?;
    let previous = state
        .index
        .lock()
        .map_err(|_| "Workspace index is unavailable.".to_string())?
        .clone();

    let needs_initial_build = previous
        .as_ref()
        .map(|index| index.root != root)
        .unwrap_or(true);

    if force || needs_initial_build {
        let next = WorkspaceIndex::build(&root, previous.as_ref());
        let mut guard = state
            .index
            .lock()
            .map_err(|_| "Workspace index is unavailable.".to_string())?;
        *guard = Some(next);
        state.refreshing.store(false, Ordering::Release);
        return Ok(root);
    }

    let stale = previous
        .as_ref()
        .map(|index| now_seconds().saturating_sub(index.built_at) >= INDEX_REFRESH_SECONDS)
        .unwrap_or(false);

    if stale
        && state
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        let root_for_thread = root.clone();
        let index_state = Arc::clone(&state.index);
        let refreshing = Arc::clone(&state.refreshing);

        thread::spawn(move || {
            let previous = index_state.lock().ok().and_then(|guard| guard.clone());
            let next = WorkspaceIndex::build(&root_for_thread, previous.as_ref());

            if let Ok(mut guard) = index_state.lock() {
                let same_workspace = guard
                    .as_ref()
                    .map(|index| index.root == root_for_thread)
                    .unwrap_or(true);

                if same_workspace {
                    *guard = Some(next);
                }
            }

            refreshing.store(false, Ordering::Release);
        });
    }

    Ok(root)
}

fn with_workspace_index<T>(
    state: &State<'_, WorkspaceState>,
    force: bool,
    operation: impl FnOnce(&WorkspaceIndex) -> T,
) -> Result<T, String> {
    refresh_workspace_index(state, force)?;

    let guard = state
        .index
        .lock()
        .map_err(|_| "Workspace index is unavailable.".to_string())?;
    let index = guard
        .as_ref()
        .ok_or_else(|| "Workspace index is not initialized.".to_string())?;

    Ok(operation(index))
}

fn read_text_file(root: &Path, relative_path: &str) -> Result<WorkspaceFile, String> {
    let path = resolve_workspace_path(root, relative_path)?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("Unable to inspect file metadata: {error}"))?;

    if !metadata.is_file() {
        return Err("The selected path is not a file.".to_string());
    }

    if metadata.len() > MAX_READ_BYTES {
        return Err("This file is too large to open in BOSCode context.".to_string());
    }

    let bytes = fs::read(&path).map_err(|error| format!("Unable to read file: {error}"))?;

    if bytes.iter().take(8_192).any(|byte| *byte == 0) {
        return Err("Binary files are not included in BOSCode context.".to_string());
    }

    let content =
        String::from_utf8(bytes).map_err(|_| "This file is not valid UTF-8 text.".to_string())?;

    Ok(WorkspaceFile {
        path: relative_path.replace('\\', "/"),
        content,
        size: metadata.len(),
        language: language_for_path(&path),
    })
}

fn context_tokens(query: &str) -> Vec<String> {
    let mut tokens = query
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            if token.len() >= 4 {
                Some(token)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    tokens.sort();
    tokens.dedup();
    tokens.truncate(12);
    tokens
}

fn build_context(
    root: &Path,
    index: &WorkspaceIndex,
    query: &str,
    active_file: Option<&str>,
) -> String {
    let summary = workspace_summary_from_index(root, index);
    let mut output = String::new();

    output.push_str("BOSCode workspace context\n");
    output.push_str(&format!("Workspace: {}\n", summary.name));
    if let Some(branch) = &summary.branch {
        output.push_str(&format!("Git branch: {branch}\n"));
    }
    output.push_str(&format!(
        "Indexed: {} files, {} directories{}\n\n",
        summary.file_count,
        summary.directory_count,
        if summary.truncated {
            " (entry list truncated)"
        } else {
            ""
        }
    ));

    output.push_str("Repository paths:\n");
    for entry in index.entries.iter().filter(|entry| !entry.is_dir).take(120) {
        output.push_str("- ");
        output.push_str(&entry.path);
        output.push('\n');
    }

    if let Some(active_file) = active_file.filter(|path| !path.trim().is_empty()) {
        if let Ok(file) = read_text_file(root, active_file) {
            output.push_str("\nActive file: ");
            output.push_str(&file.path);
            output.push_str("\n~~~");
            output.push_str(&file.language.to_ascii_lowercase());
            output.push('\n');
            output.extend(file.content.chars().take(18_000));
            output.push_str("\n~~~\n");
        }
    }

    let tokens = context_tokens(query);
    let snippets = index.rank_snippets(&tokens, 16);
    if !snippets.is_empty() {
        output.push_str("\nRelevant repository snippets (cached rank):\n");
        for snippet in snippets {
            output.push_str(&format!(
                "- {}:{} [score {}]: {}\n",
                snippet.path, snippet.line, snippet.score, snippet.preview
            ));
        }
    }

    output.chars().take(MAX_CONTEXT_CHARS).collect()
}

#[tauri::command]
pub fn set_workspace(
    path: String,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceSummary, String> {
    let canonical = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| "The selected workspace folder does not exist.".to_string())?;

    if !canonical.is_dir() {
        return Err("Select a folder to use as the BOSCode workspace.".to_string());
    }

    {
        let mut root = state
            .root
            .lock()
            .map_err(|_| "Workspace state is unavailable.".to_string())?;
        *root = Some(canonical.clone());
    }

    let next_index = WorkspaceIndex::build(&canonical, None);
    let summary = workspace_summary_from_index(&canonical, &next_index);
    {
        let mut index = state
            .index
            .lock()
            .map_err(|_| "Workspace index is unavailable.".to_string())?;
        *index = Some(next_index);
    }
    state.refreshing.store(false, Ordering::Release);

    Ok(summary)
}

#[tauri::command]
pub fn get_workspace(state: State<'_, WorkspaceState>) -> Result<Option<WorkspaceSummary>, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Workspace state is unavailable.".to_string())?
        .clone();

    let Some(root) = root else {
        return Ok(None);
    };

    let summary = with_workspace_index(&state, false, |index| {
        workspace_summary_from_index(&root, index)
    })?;
    Ok(Some(summary))
}

#[tauri::command]
pub fn list_workspace(state: State<'_, WorkspaceState>) -> Result<Vec<WorkspaceEntry>, String> {
    with_workspace_index(&state, false, |index| index.entries.clone())
}

#[tauri::command]
pub fn read_workspace_file(
    path: String,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceFile, String> {
    let root = workspace_root(&state)?;
    read_text_file(&root, &path)
}

#[tauri::command]
pub fn search_workspace(
    query: String,
    max_results: Option<usize>,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<SearchHit>, String> {
    let limit = max_results.unwrap_or(80).clamp(1, 200);
    with_workspace_index(&state, false, |index| index.search(&query, limit))
}

#[tauri::command]
pub fn build_workspace_context(
    query: String,
    active_file: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<String, String> {
    let root = refresh_workspace_index(&state, false)?;
    with_workspace_index(&state, false, |index| {
        build_context(&root, index, &query, active_file.as_deref())
    })
}

#[cfg(test)]
mod tests {
    use super::{context_tokens, is_sensitive_path};
    use std::path::Path;

    #[test]
    fn blocks_common_secret_files_but_allows_examples() {
        assert!(is_sensitive_path(Path::new(".env")));
        assert!(is_sensitive_path(Path::new(".env.production")));
        assert!(is_sensitive_path(Path::new("id_rsa")));
        assert!(is_sensitive_path(Path::new("server.key")));
        assert!(!is_sensitive_path(Path::new(".env.example")));
        assert!(!is_sensitive_path(Path::new("config.ts")));
    }

    #[test]
    fn context_tokens_are_normalized_deduplicated_and_bounded() {
        let tokens = context_tokens(
            "Fix Checkout checkout rounding in Invoice.php and CheckoutTest with repository context",
        );

        assert!(tokens.contains(&"checkout".to_string()));
        assert!(tokens.contains(&"rounding".to_string()));
        assert!(tokens.contains(&"invoice".to_string()));
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.as_str() == "checkout")
                .count(),
            1
        );
        assert!(tokens.len() <= 12);
    }
}
