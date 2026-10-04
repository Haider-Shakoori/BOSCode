use crate::workspace::{is_sensitive_path, workspace_root, WorkspaceState};
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

const MAX_CHANGE_BYTES: usize = 1_500_000;
const MAX_PENDING_CHANGES: usize = 100;
const MAX_UNDO_RECORDS: usize = 50;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PermissionMode {
    #[default]
    ReadOnly,
    WorkspaceWrite,
}

impl PermissionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read-only" => Ok(Self::ReadOnly),
            "workspace-write" => Ok(Self::WorkspaceWrite),
            _ => Err("Unsupported permission mode.".to_string()),
        }
    }
}

#[derive(Default)]
pub struct ChangeState {
    permission: Mutex<PermissionMode>,
    pending: Mutex<HashMap<String, PendingChange>>,
    undo: Mutex<Vec<UndoRecord>>,
    sequence: AtomicU64,
}

#[derive(Clone)]
struct PendingChange {
    id: String,
    path: String,
    action: ChangeAction,
    original: Option<String>,
    proposed: Option<String>,
    diff: String,
    additions: usize,
    deletions: usize,
}

#[derive(Clone, Copy)]
enum ChangeAction {
    Create,
    Update,
    Delete,
}

impl ChangeAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

struct UndoRecord {
    path: String,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeView {
    id: String,
    path: String,
    action: String,
    original_content: Option<String>,
    proposed_content: Option<String>,
    diff: String,
    additions: usize,
    deletions: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    mode: String,
    can_write: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    path: String,
    action: String,
    undo_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    path: String,
    restored: bool,
    undo_available: bool,
}

fn pending_view(change: &PendingChange) -> PendingChangeView {
    PendingChangeView {
        id: change.id.clone(),
        path: change.path.clone(),
        action: change.action.as_str().to_string(),
        original_content: change.original.clone(),
        proposed_content: change.proposed.clone(),
        diff: change.diff.clone(),
        additions: change.additions,
        deletions: change.deletions,
    }
}

fn ensure_write_permission(state: &State<'_, ChangeState>) -> Result<(), String> {
    let mode = *state
        .permission
        .lock()
        .map_err(|_| "Permission state is unavailable.".to_string())?;

    if mode == PermissionMode::WorkspaceWrite {
        Ok(())
    } else {
        Err(
            "BOSCode is in read-only mode. Enable Workspace Write before applying changes."
                .to_string(),
        )
    }
}

fn validate_relative_path(relative_path: &str) -> Result<&Path, String> {
    let relative = Path::new(relative_path);

    if relative_path.trim().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("The requested path is outside the workspace.".to_string());
    }

    if is_sensitive_path(relative) {
        return Err("BOSCode blocks changes to sensitive credential files.".to_string());
    }

    Ok(relative)
}

fn resolve_change_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = validate_relative_path(relative_path)?;
    let candidate = root.join(relative);

    if candidate.exists() {
        if fs::symlink_metadata(&candidate)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err("BOSCode does not modify files through symbolic links.".to_string());
        }

        let canonical = candidate
            .canonicalize()
            .map_err(|_| "Unable to resolve the requested path.".to_string())?;

        if !canonical.starts_with(root) {
            return Err("The requested path is outside the workspace.".to_string());
        }

        if canonical.is_dir() {
            return Err("BOSCode file changes cannot target a directory.".to_string());
        }

        if is_sensitive_path(&canonical) {
            return Err("BOSCode blocks changes to sensitive credential files.".to_string());
        }

        return Ok(canonical);
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| "The requested file has no parent directory.".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "Create the parent folder before proposing this file.".to_string())?;

    if !canonical_parent.starts_with(root) {
        return Err("The requested path is outside the workspace.".to_string());
    }

    let file_name = candidate
        .file_name()
        .ok_or_else(|| "The requested file name is invalid.".to_string())?;

    Ok(canonical_parent.join(file_name))
}

fn read_optional_text(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let metadata =
        fs::metadata(path).map_err(|error| format!("Unable to inspect current file: {error}"))?;

    if !metadata.is_file() {
        return Err("The selected path is not a file.".to_string());
    }

    if metadata.len() as usize > MAX_CHANGE_BYTES {
        return Err("This file is too large for BOSCode changes.".to_string());
    }

    let bytes = fs::read(path).map_err(|error| format!("Unable to read current file: {error}"))?;
    if bytes.iter().take(8_192).any(|byte| *byte == 0) {
        return Err("BOSCode does not edit binary files.".to_string());
    }

    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "BOSCode only edits UTF-8 text files.".to_string())
}

fn diff_for(before: Option<&str>, after: Option<&str>, path: &str) -> (String, usize, usize) {
    let before = before.unwrap_or("");
    let after = after.unwrap_or("");
    let diff = TextDiff::from_lines(before, after);
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Insert => additions += 1,
            ChangeTag::Equal => {}
        }
    }

    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();

    (unified, additions, deletions)
}

fn next_change_id(state: &ChangeState) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    format!("change-{millis}-{sequence}")
}

fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if content.len() > MAX_CHANGE_BYTES {
        return Err("The proposed file is too large for BOSCode changes.".to_string());
    }

    fs::write(path, content.as_bytes())
        .map_err(|error| format!("Unable to write workspace file: {error}"))
}

#[tauri::command]
pub fn get_permission_mode(state: State<'_, ChangeState>) -> Result<PermissionStatus, String> {
    let mode = *state
        .permission
        .lock()
        .map_err(|_| "Permission state is unavailable.".to_string())?;

    Ok(PermissionStatus {
        mode: mode.as_str().to_string(),
        can_write: mode == PermissionMode::WorkspaceWrite,
    })
}

#[tauri::command]
pub fn set_permission_mode(
    mode: String,
    state: State<'_, ChangeState>,
) -> Result<PermissionStatus, String> {
    let parsed = PermissionMode::parse(&mode)?;

    let mut current = state
        .permission
        .lock()
        .map_err(|_| "Permission state is unavailable.".to_string())?;
    *current = parsed;

    Ok(PermissionStatus {
        mode: parsed.as_str().to_string(),
        can_write: parsed == PermissionMode::WorkspaceWrite,
    })
}

#[tauri::command]
pub fn propose_workspace_change(
    path: String,
    proposed_content: Option<String>,
    delete_file: Option<bool>,
    workspace: State<'_, WorkspaceState>,
    changes: State<'_, ChangeState>,
) -> Result<PendingChangeView, String> {
    let root = workspace_root(&workspace)?;
    let target = resolve_change_path(&root, &path)?;
    let original = read_optional_text(&target)?;
    let delete_file = delete_file.unwrap_or(false);

    let (action, proposed) = if delete_file {
        if original.is_none() {
            return Err("Cannot delete a file that does not exist.".to_string());
        }
        (ChangeAction::Delete, None)
    } else {
        let content =
            proposed_content.ok_or_else(|| "Proposed content is required.".to_string())?;

        if content.len() > MAX_CHANGE_BYTES {
            return Err("The proposed file is too large for BOSCode changes.".to_string());
        }

        let action = if original.is_some() {
            ChangeAction::Update
        } else {
            ChangeAction::Create
        };
        (action, Some(content))
    };

    if original == proposed {
        return Err("The proposed content is identical to the current file.".to_string());
    }

    let normalized_path = path.replace('\\', "/");
    let (diff, additions, deletions) =
        diff_for(original.as_deref(), proposed.as_deref(), &normalized_path);

    let pending = PendingChange {
        id: next_change_id(&changes),
        path: normalized_path,
        action,
        original,
        proposed,
        diff,
        additions,
        deletions,
    };

    let view = pending_view(&pending);
    let mut pending_changes = changes
        .pending
        .lock()
        .map_err(|_| "Change state is unavailable.".to_string())?;

    pending_changes.retain(|_, existing| existing.path != pending.path);

    if pending_changes.len() >= MAX_PENDING_CHANGES {
        return Err("Too many pending changes. Apply or reject some changes first.".to_string());
    }

    pending_changes.insert(pending.id.clone(), pending);
    Ok(view)
}

#[tauri::command]
pub fn list_pending_changes(
    state: State<'_, ChangeState>,
) -> Result<Vec<PendingChangeView>, String> {
    let pending = state
        .pending
        .lock()
        .map_err(|_| "Change state is unavailable.".to_string())?;

    let mut items = pending.values().map(pending_view).collect::<Vec<_>>();
    items.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(items)
}

#[tauri::command]
pub fn reject_workspace_change(
    change_id: String,
    state: State<'_, ChangeState>,
) -> Result<bool, String> {
    let removed = state
        .pending
        .lock()
        .map_err(|_| "Change state is unavailable.".to_string())?
        .remove(&change_id)
        .is_some();

    Ok(removed)
}

#[tauri::command]
pub fn apply_workspace_change(
    change_id: String,
    workspace: State<'_, WorkspaceState>,
    changes: State<'_, ChangeState>,
) -> Result<ApplyResult, String> {
    ensure_write_permission(&changes)?;

    let root = workspace_root(&workspace)?;
    let pending = changes
        .pending
        .lock()
        .map_err(|_| "Change state is unavailable.".to_string())?
        .get(&change_id)
        .cloned()
        .ok_or_else(|| "This pending change no longer exists.".to_string())?;

    let target = resolve_change_path(&root, &pending.path)?;
    let current = read_optional_text(&target)?;

    if current != pending.original {
        return Err(
            "The file changed after this proposal was created. Refresh the proposal before applying."
                .to_string(),
        );
    }

    match pending.action {
        ChangeAction::Create | ChangeAction::Update => {
            write_text(
                &target,
                pending
                    .proposed
                    .as_deref()
                    .ok_or_else(|| "Proposed content is missing.".to_string())?,
            )?;
        }
        ChangeAction::Delete => {
            fs::remove_file(&target)
                .map_err(|error| format!("Unable to delete workspace file: {error}"))?;
        }
    }

    {
        let mut undo = changes
            .undo
            .lock()
            .map_err(|_| "Undo state is unavailable.".to_string())?;
        undo.push(UndoRecord {
            path: pending.path.clone(),
            before: pending.original.clone(),
            after: pending.proposed.clone(),
        });
        if undo.len() > MAX_UNDO_RECORDS {
            undo.remove(0);
        }
    }

    changes
        .pending
        .lock()
        .map_err(|_| "Change state is unavailable.".to_string())?
        .remove(&change_id);

    let undo_available = !changes
        .undo
        .lock()
        .map_err(|_| "Undo state is unavailable.".to_string())?
        .is_empty();

    Ok(ApplyResult {
        path: pending.path,
        action: pending.action.as_str().to_string(),
        undo_available,
    })
}

#[tauri::command]
pub fn undo_last_workspace_change(
    workspace: State<'_, WorkspaceState>,
    changes: State<'_, ChangeState>,
) -> Result<UndoResult, String> {
    ensure_write_permission(&changes)?;
    let root = workspace_root(&workspace)?;

    let record = {
        let mut undo = changes
            .undo
            .lock()
            .map_err(|_| "Undo state is unavailable.".to_string())?;
        undo.pop()
            .ok_or_else(|| "There is no BOSCode change to undo.".to_string())?
    };

    let target = resolve_change_path(&root, &record.path)?;
    let current = read_optional_text(&target)?;

    if current != record.after {
        changes
            .undo
            .lock()
            .map_err(|_| "Undo state is unavailable.".to_string())?
            .push(record);
        return Err(
            "The file changed after BOSCode applied it. Undo was stopped to protect newer work."
                .to_string(),
        );
    }

    match record.before.as_deref() {
        Some(content) => write_text(&target, content)?,
        None => {
            if target.exists() {
                fs::remove_file(&target)
                    .map_err(|error| format!("Unable to undo created file: {error}"))?;
            }
        }
    }

    let undo_available = !changes
        .undo
        .lock()
        .map_err(|_| "Undo state is unavailable.".to_string())?
        .is_empty();

    Ok(UndoResult {
        path: record.path,
        restored: true,
        undo_available,
    })
}

#[cfg(test)]
mod tests {
    use super::{diff_for, validate_relative_path, PermissionMode};

    #[test]
    fn permission_mode_defaults_to_read_only_and_parses_write_mode() {
        assert_eq!(PermissionMode::default().as_str(), "read-only");
        assert_eq!(
            PermissionMode::parse("workspace-write").unwrap().as_str(),
            "workspace-write"
        );
        assert!(PermissionMode::parse("full-access").is_err());
    }

    #[test]
    fn rejects_escape_and_sensitive_paths() {
        assert!(validate_relative_path("../outside.txt").is_err());
        assert!(validate_relative_path("C:\\Windows\\system.ini").is_err());
        assert!(validate_relative_path(".env").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn creates_unified_diff_and_counts_lines() {
        let (diff, additions, deletions) =
            diff_for(Some("one\ntwo\n"), Some("one\nthree\n"), "src/test.txt");

        assert!(diff.contains("--- a/src/test.txt"));
        assert!(diff.contains("+++ b/src/test.txt"));
        assert_eq!(additions, 1);
        assert_eq!(deletions, 1);
    }
}
