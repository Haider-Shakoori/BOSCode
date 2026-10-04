use serde::Serialize;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};
use tauri::State;
use walkdir::{DirEntry, WalkDir};

const MAX_WORKSPACE_ENTRIES: usize = 6_000;
const MAX_READ_BYTES: u64 = 1_500_000;
const MAX_SEARCH_BYTES: u64 = 1_000_000;
const MAX_CONTEXT_CHARS: usize = 32_000;

#[derive(Default)]
pub struct WorkspaceState {
    root: Mutex<Option<PathBuf>>,
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
        entry.file_name().to_string_lossy().to_ascii_lowercase().as_str(),
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

fn is_sensitive_path(path: &Path) -> bool {
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

fn workspace_root(state: &State<'_, WorkspaceState>) -> Result<PathBuf, String> {
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

fn scan_workspace(root: &Path) -> (Vec<WorkspaceEntry>, usize, usize, bool) {
    let mut entries = Vec::new();
    let mut file_count = 0usize;
    let mut directory_count = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !ignored_entry(entry))
        .filter_map(Result::ok)
        .skip(1)
    {
        if entries.len() >= MAX_WORKSPACE_ENTRIES {
            truncated = true;
            break;
        }

        if entry.file_type().is_symlink() || is_sensitive_path(entry.path()) {
            continue;
        }

        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };

        let is_dir = entry.file_type().is_dir();
        if is_dir {
            directory_count += 1;
        } else if entry.file_type().is_file() {
            file_count += 1;
        } else {
            continue;
        }

        entries.push(WorkspaceEntry {
            path: relative_display(relative),
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir,
            depth: relative.components().count().saturating_sub(1),
            size: if is_dir {
                None
            } else {
                entry.metadata().ok().map(|metadata| metadata.len())
            },
        });
    }

    entries.sort_by(|left, right| {
        left.path
            .to_ascii_lowercase()
            .cmp(&right.path.to_ascii_lowercase())
    });

    (entries, file_count, directory_count, truncated)
}

fn workspace_summary(root: &Path) -> WorkspaceSummary {
    let (_, file_count, directory_count, truncated) = scan_workspace(root);

    WorkspaceSummary {
        root: root.to_string_lossy().to_string(),
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace")
            .to_string(),
        branch: read_git_branch(root),
        file_count,
        directory_count,
        truncated,
    }
}

fn read_text_file(root: &Path, relative_path: &str) -> Result<WorkspaceFile, String> {
    let path = resolve_workspace_path(root, relative_path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Unable to inspect file metadata: {error}"))?;

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

fn search_root(root: &Path, query: &str, limit: usize) -> Vec<SearchHit> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.len() < 2 {
        return Vec::new();
    }

    let mut hits = Vec::new();

    'files: for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !ignored_entry(entry))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file()
            || entry.file_type().is_symlink()
            || is_sensitive_path(entry.path())
        {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        if metadata.len() > MAX_SEARCH_BYTES {
            continue;
        }

        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };

        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            continue;
        }

        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };

        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            if line.to_ascii_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    path: relative_display(relative),
                    line: index + 1,
                    preview: line.trim().chars().take(240).collect(),
                });

                if hits.len() >= limit {
                    break 'files;
                }
            }
        }
    }

    hits
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

fn build_context(root: &Path, query: &str, active_file: Option<&str>) -> String {
    let summary = workspace_summary(root);
    let (entries, _, _, _) = scan_workspace(root);
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
    for entry in entries.iter().filter(|entry| !entry.is_dir).take(120) {
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
    if !tokens.is_empty() {
        output.push_str("\nRelevant repository snippets:\n");
        let mut snippet_count = 0usize;

        for token in tokens {
            for hit in search_root(root, &token, 4) {
                output.push_str(&format!("- {}:{}: {}\n", hit.path, hit.line, hit.preview));
                snippet_count += 1;

                if snippet_count >= 16 {
                    break;
                }
            }

            if snippet_count >= 16 {
                break;
            }
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

    Ok(workspace_summary(&canonical))
}

#[tauri::command]
pub fn get_workspace(
    state: State<'_, WorkspaceState>,
) -> Result<Option<WorkspaceSummary>, String> {
    let root = state
        .root
        .lock()
        .map_err(|_| "Workspace state is unavailable.".to_string())?
        .clone();

    Ok(root.map(|path| workspace_summary(&path)))
}

#[tauri::command]
pub fn list_workspace(
    state: State<'_, WorkspaceState>,
) -> Result<Vec<WorkspaceEntry>, String> {
    let root = workspace_root(&state)?;
    let (entries, _, _, _) = scan_workspace(&root);
    Ok(entries)
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
    let root = workspace_root(&state)?;
    let limit = max_results.unwrap_or(80).clamp(1, 200);
    Ok(search_root(&root, &query, limit))
}

#[tauri::command]
pub fn build_workspace_context(
    query: String,
    active_file: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<String, String> {
    let root = workspace_root(&state)?;
    Ok(build_context(&root, &query, active_file.as_deref()))
}
