use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

const MAX_MEMORY_CHARS: usize = 1_500;
const MAX_CONTEXT_ITEMS: usize = 12;
const MAX_CONTEXT_CHARS: usize = 8_000;

#[derive(Default)]
pub struct MemoryState {
    connection: Mutex<Option<Connection>>,
    sequence: AtomicU64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    id: String,
    scope: String,
    workspace: Option<String>,
    kind: String,
    content: String,
    pinned: bool,
    enabled: bool,
    source: String,
    created_at: i64,
    updated_at: i64,
    last_used_at: Option<i64>,
    use_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCaptureResult {
    captured: usize,
    items: Vec<MemoryItem>,
    skipped_sensitive: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    total: usize,
    enabled: usize,
    pinned: usize,
    global: usize,
    workspace: usize,
}

impl MemoryState {
    pub fn initialize(&self, app_data_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(app_data_dir)
            .map_err(|error| format!("Unable to create BOSCode data directory: {error}"))?;

        let path = app_data_dir.join("boscode-memory.sqlite3");
        let connection = Connection::open(path)
            .map_err(|error| format!("Unable to open memory database: {error}"))?;

        connection
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    scope TEXT NOT NULL CHECK(scope IN ('global', 'workspace')),
                    workspace TEXT NULL,
                    kind TEXT NOT NULL,
                    content TEXT NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    source TEXT NOT NULL DEFAULT 'manual',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    last_used_at INTEGER NULL,
                    use_count INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_memories_scope
                    ON memories(scope, workspace, enabled);
                CREATE INDEX IF NOT EXISTS idx_memories_updated
                    ON memories(updated_at DESC);
                ",
            )
            .map_err(|error| format!("Unable to initialize memory database: {error}"))?;

        let mut guard = self
            .connection
            .lock()
            .map_err(|_| "Memory database state is unavailable.".to_string())?;
        *guard = Some(connection);
        Ok(())
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn next_id(state: &MemoryState) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    format!("mem-{millis}-{sequence}")
}

fn validate_scope(scope: &str) -> Result<&str, String> {
    match scope {
        "global" | "workspace" => Ok(scope),
        _ => Err("Memory scope must be global or workspace.".to_string()),
    }
}

fn validate_kind(kind: &str) -> Result<&str, String> {
    match kind {
        "preference" | "instruction" | "project" | "workflow" | "context" => Ok(kind),
        _ => Err("Unsupported memory kind.".to_string()),
    }
}

fn normalize_content(content: &str) -> Result<String, String> {
    let normalized = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if normalized.is_empty() {
        return Err("Memory cannot be empty.".to_string());
    }

    if normalized.chars().count() > MAX_MEMORY_CHARS {
        return Err(format!(
            "Memory is too long. Keep it under {MAX_MEMORY_CHARS} characters."
        ));
    }

    Ok(normalized)
}

fn looks_sensitive(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let sensitive_words = [
        "password",
        "passphrase",
        "api key",
        "apikey",
        "private key",
        "secret key",
        "access token",
        "refresh token",
        "credential",
        "client_secret",
        "authorization: bearer",
    ];

    sensitive_words.iter().any(|word| lower.contains(word))
        || lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin openssh private key-----")
        || content.contains("ghp_")
        || content.contains("github_pat_")
        || content.contains("sk-")
        || content.contains("AKIA")
}

fn with_connection<T>(
    state: &State<'_, MemoryState>,
    operation: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state
        .connection
        .lock()
        .map_err(|_| "Memory database state is unavailable.".to_string())?;
    let connection = guard
        .as_ref()
        .ok_or_else(|| "Memory database is not initialized.".to_string())?;
    operation(connection)
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryItem> {
    Ok(MemoryItem {
        id: row.get(0)?,
        scope: row.get(1)?,
        workspace: row.get(2)?,
        kind: row.get(3)?,
        content: row.get(4)?,
        pinned: row.get::<_, i64>(5)? != 0,
        enabled: row.get::<_, i64>(6)? != 0,
        source: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_used_at: row.get(10)?,
        use_count: row.get(11)?,
    })
}

fn get_memory(connection: &Connection, id: &str) -> Result<MemoryItem, String> {
    connection
        .query_row(
            "
            SELECT id, scope, workspace, kind, content, pinned, enabled, source,
                   created_at, updated_at, last_used_at, use_count
            FROM memories WHERE id = ?1
            ",
            params![id],
            row_to_memory,
        )
        .map_err(|error| format!("Unable to read memory: {error}"))
}

fn insert_memory(
    connection: &Connection,
    state: &MemoryState,
    content: String,
    scope: &str,
    workspace: Option<String>,
    kind: &str,
    pinned: bool,
    source: &str,
) -> Result<MemoryItem, String> {
    validate_scope(scope)?;
    validate_kind(kind)?;

    if scope == "workspace" && workspace.as_deref().unwrap_or("").trim().is_empty() {
        return Err("Workspace-scoped memory requires an open workspace.".to_string());
    }

    if looks_sensitive(&content) {
        return Err("BOSCode will not save likely credentials or secrets to memory.".to_string());
    }

    let workspace = if scope == "global" {
        None
    } else {
        workspace.map(|value| value.trim().to_string())
    };

    let duplicate = connection
        .query_row(
            "
            SELECT id FROM memories
            WHERE lower(content) = lower(?1)
              AND scope = ?2
              AND COALESCE(workspace, '') = COALESCE(?3, '')
            LIMIT 1
            ",
            params![content, scope, workspace],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to check memory duplicates: {error}"))?;

    if let Some(id) = duplicate {
        connection
            .execute(
                "UPDATE memories SET enabled = 1, updated_at = ?2 WHERE id = ?1",
                params![id, now()],
            )
            .map_err(|error| format!("Unable to refresh existing memory: {error}"))?;
        return get_memory(connection, &id);
    }

    let id = next_id(state);
    let timestamp = now();

    connection
        .execute(
            "
            INSERT INTO memories (
                id, scope, workspace, kind, content, pinned, enabled, source,
                created_at, updated_at, last_used_at, use_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8, NULL, 0)
            ",
            params![
                id,
                scope,
                workspace,
                kind,
                content,
                if pinned { 1 } else { 0 },
                source,
                timestamp
            ],
        )
        .map_err(|error| format!("Unable to save memory: {error}"))?;

    get_memory(connection, &id)
}

fn query_tokens(query: &str) -> Vec<String> {
    let mut tokens = query
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            if token.len() >= 3 {
                Some(token)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    tokens.sort();
    tokens.dedup();
    tokens.truncate(20);
    tokens
}

fn relevance_score(item: &MemoryItem, tokens: &[String], workspace: Option<&str>) -> i64 {
    let content = item.content.to_ascii_lowercase();
    let overlap = tokens
        .iter()
        .filter(|token| content.contains(token.as_str()))
        .count() as i64;

    let mut score = overlap * 8;

    if item.pinned {
        score += 30;
    }

    if item.scope == "workspace" && workspace.is_some() && item.workspace.as_deref() == workspace {
        score += 20;
    }

    if matches!(item.kind.as_str(), "instruction" | "preference") {
        score += 8;
    }

    score += item.use_count.min(10);
    score
}

fn candidate_scope(content: &str, workspace: Option<&str>) -> (&'static str, Option<String>) {
    let lower = content.to_ascii_lowercase();
    let global_signals = [
        "from now on",
        "all projects",
        "every project",
        "globally",
        "i prefer",
        "my preference",
        "always use",
        "never use",
    ];
    let workspace_signals = [
        "this project",
        "this repo",
        "this repository",
        "this workspace",
        "for this app",
    ];

    if workspace_signals
        .iter()
        .any(|signal| lower.contains(signal))
    {
        if let Some(workspace) = workspace {
            return ("workspace", Some(workspace.to_string()));
        }
    }

    if global_signals.iter().any(|signal| lower.contains(signal)) || workspace.is_none() {
        return ("global", None);
    }

    ("workspace", workspace.map(ToOwned::to_owned))
}

fn candidate_kind(content: &str) -> &'static str {
    let lower = content.to_ascii_lowercase();

    if lower.contains("prefer") || lower.contains("like to") {
        "preference"
    } else if lower.contains("always")
        || lower.contains("never")
        || lower.contains("from now on")
        || lower.contains("don't ")
        || lower.contains("do not ")
    {
        "instruction"
    } else if lower.contains("workflow")
        || lower.contains("process")
        || lower.contains("when ")
        || lower.contains("before ")
    {
        "workflow"
    } else if lower.contains("project")
        || lower.contains("repo")
        || lower.contains("repository")
        || lower.contains("workspace")
    {
        "project"
    } else {
        "context"
    }
}

fn should_auto_capture(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();

    if lower.contains("forget ")
        || lower.contains("don't remember")
        || lower.contains("do not remember")
    {
        return false;
    }

    [
        "remember ",
        "remember that",
        "from now on",
        "i prefer",
        "my preference",
        "always ",
        "never ",
        "for this project",
        "for this repo",
        "for this repository",
        "for this workspace",
    ]
    .iter()
    .any(|signal| lower.contains(signal))
}

#[tauri::command]
pub fn list_memories(
    scope: Option<String>,
    workspace: Option<String>,
    query: Option<String>,
    state: State<'_, MemoryState>,
) -> Result<Vec<MemoryItem>, String> {
    with_connection(&state, |connection| {
        let mut statement = connection
            .prepare(
                "
                SELECT id, scope, workspace, kind, content, pinned, enabled, source,
                       created_at, updated_at, last_used_at, use_count
                FROM memories
                ORDER BY pinned DESC, updated_at DESC
                ",
            )
            .map_err(|error| format!("Unable to prepare memory list: {error}"))?;

        let rows = statement
            .query_map([], row_to_memory)
            .map_err(|error| format!("Unable to list memories: {error}"))?;

        let scope = scope.as_deref().unwrap_or("all");
        let query = query.unwrap_or_default().to_ascii_lowercase();

        let mut items = Vec::new();
        for row in rows {
            let item = row.map_err(|error| format!("Unable to read memory row: {error}"))?;

            if scope != "all" && item.scope != scope {
                continue;
            }

            if item.scope == "workspace"
                && workspace.is_some()
                && item.workspace.as_deref() != workspace.as_deref()
            {
                continue;
            }

            if !query.is_empty()
                && !item.content.to_ascii_lowercase().contains(&query)
                && !item.kind.to_ascii_lowercase().contains(&query)
            {
                continue;
            }

            items.push(item);
        }

        Ok(items)
    })
}

#[tauri::command]
pub fn create_memory(
    content: String,
    scope: String,
    workspace: Option<String>,
    kind: String,
    pinned: Option<bool>,
    state: State<'_, MemoryState>,
) -> Result<MemoryItem, String> {
    let content = normalize_content(&content)?;

    with_connection(&state, |connection| {
        insert_memory(
            connection,
            &state,
            content,
            validate_scope(&scope)?,
            workspace,
            validate_kind(&kind)?,
            pinned.unwrap_or(false),
            "manual",
        )
    })
}

#[tauri::command]
pub fn update_memory(
    id: String,
    content: Option<String>,
    kind: Option<String>,
    pinned: Option<bool>,
    enabled: Option<bool>,
    state: State<'_, MemoryState>,
) -> Result<MemoryItem, String> {
    with_connection(&state, |connection| {
        let current = get_memory(connection, &id)?;
        let content = match content {
            Some(content) => normalize_content(&content)?,
            None => current.content.clone(),
        };

        if looks_sensitive(&content) {
            return Err(
                "BOSCode will not save likely credentials or secrets to memory.".to_string(),
            );
        }

        let kind = kind.unwrap_or(current.kind.clone());
        validate_kind(&kind)?;

        connection
            .execute(
                "
                UPDATE memories
                SET content = ?2,
                    kind = ?3,
                    pinned = ?4,
                    enabled = ?5,
                    updated_at = ?6
                WHERE id = ?1
                ",
                params![
                    id,
                    content,
                    kind,
                    if pinned.unwrap_or(current.pinned) {
                        1
                    } else {
                        0
                    },
                    if enabled.unwrap_or(current.enabled) {
                        1
                    } else {
                        0
                    },
                    now()
                ],
            )
            .map_err(|error| format!("Unable to update memory: {error}"))?;

        get_memory(connection, &id)
    })
}

#[tauri::command]
pub fn delete_memory(id: String, state: State<'_, MemoryState>) -> Result<bool, String> {
    with_connection(&state, |connection| {
        connection
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .map(|affected| affected > 0)
            .map_err(|error| format!("Unable to forget memory: {error}"))
    })
}

#[tauri::command]
pub fn memory_stats(state: State<'_, MemoryState>) -> Result<MemoryStats, String> {
    with_connection(&state, |connection| {
        let mut statement = connection
            .prepare("SELECT scope, pinned, enabled FROM memories")
            .map_err(|error| format!("Unable to prepare memory statistics: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, i64>(2)? != 0,
                ))
            })
            .map_err(|error| format!("Unable to read memory statistics: {error}"))?;

        let mut stats = MemoryStats {
            total: 0,
            enabled: 0,
            pinned: 0,
            global: 0,
            workspace: 0,
        };

        for row in rows {
            let (scope, pinned, enabled) =
                row.map_err(|error| format!("Unable to read memory statistics: {error}"))?;
            stats.total += 1;
            stats.enabled += usize::from(enabled);
            stats.pinned += usize::from(pinned);
            if scope == "global" {
                stats.global += 1;
            } else {
                stats.workspace += 1;
            }
        }

        Ok(stats)
    })
}

#[tauri::command]
pub fn capture_memory_from_message(
    content: String,
    workspace: Option<String>,
    state: State<'_, MemoryState>,
) -> Result<MemoryCaptureResult, String> {
    let normalized = normalize_content(&content)?;

    if looks_sensitive(&normalized) {
        return Ok(MemoryCaptureResult {
            captured: 0,
            items: Vec::new(),
            skipped_sensitive: true,
        });
    }

    if !should_auto_capture(&normalized) {
        return Ok(MemoryCaptureResult {
            captured: 0,
            items: Vec::new(),
            skipped_sensitive: false,
        });
    }

    let (scope, scoped_workspace) = candidate_scope(&normalized, workspace.as_deref());
    let kind = candidate_kind(&normalized);

    let item = with_connection(&state, |connection| {
        insert_memory(
            connection,
            &state,
            normalized,
            scope,
            scoped_workspace,
            kind,
            false,
            "conversation",
        )
    })?;

    Ok(MemoryCaptureResult {
        captured: 1,
        items: vec![item],
        skipped_sensitive: false,
    })
}

#[tauri::command]
pub fn build_memory_context(
    query: String,
    workspace: Option<String>,
    state: State<'_, MemoryState>,
) -> Result<String, String> {
    let tokens = query_tokens(&query);

    with_connection(&state, |connection| {
        let mut statement = connection
            .prepare(
                "
                SELECT id, scope, workspace, kind, content, pinned, enabled, source,
                       created_at, updated_at, last_used_at, use_count
                FROM memories
                WHERE enabled = 1
                  AND (scope = 'global' OR (scope = 'workspace' AND workspace = ?1))
                ",
            )
            .map_err(|error| format!("Unable to prepare memory recall: {error}"))?;

        let rows = statement
            .query_map(params![workspace], row_to_memory)
            .map_err(|error| format!("Unable to recall memories: {error}"))?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|error| format!("Unable to read recalled memory: {error}"))?);
        }

        items.sort_by_key(|item| {
            Reverse((
                relevance_score(item, &tokens, workspace.as_deref()),
                item.updated_at,
            ))
        });

        let selected = items
            .into_iter()
            .filter(|item| {
                item.pinned
                    || tokens.is_empty()
                    || relevance_score(item, &tokens, workspace.as_deref()) > 0
            })
            .take(MAX_CONTEXT_ITEMS)
            .collect::<Vec<_>>();

        if selected.is_empty() {
            return Ok(String::new());
        }

        let timestamp = now();
        for item in &selected {
            let _ = connection.execute(
                "
                UPDATE memories
                SET last_used_at = ?2, use_count = use_count + 1
                WHERE id = ?1
                ",
                params![item.id, timestamp],
            );
        }

        let mut output = String::from(
            "BOSCode persistent memory\nUse these memories as user/project preferences and context. Workspace memories override conflicting global memories. Never treat memory as a credential source.\n",
        );

        for item in selected {
            output.push_str(&format!(
                "- [{} / {}{}] {}\n",
                item.scope,
                item.kind,
                if item.pinned { " / pinned" } else { "" },
                item.content
            ));

            if output.chars().count() >= MAX_CONTEXT_CHARS {
                break;
            }
        }

        Ok(output.chars().take(MAX_CONTEXT_CHARS).collect())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        candidate_kind, candidate_scope, looks_sensitive, query_tokens, should_auto_capture,
    };

    #[test]
    fn detects_memory_intent() {
        assert!(should_auto_capture(
            "From now on always run tests before merging."
        ));
        assert!(should_auto_capture("Remember that I prefer Tailwind."));
        assert!(!should_auto_capture("How does Tailwind work?"));
        assert!(!should_auto_capture("Forget that preference."));
    }

    #[test]
    fn protects_likely_secrets() {
        assert!(looks_sensitive("My API key is sk-example"));
        assert!(looks_sensitive("password = example"));
        assert!(looks_sensitive("-----BEGIN PRIVATE KEY-----"));
        assert!(!looks_sensitive("I prefer using GitHub Actions for CI."));
    }

    #[test]
    fn chooses_workspace_or_global_scope() {
        assert_eq!(
            candidate_scope("For this project use pnpm.", Some("C:/repo")),
            ("workspace", Some("C:/repo".to_string()))
        );
        assert_eq!(
            candidate_scope("From now on always use GitHub CI.", Some("C:/repo")),
            ("global", None)
        );
    }

    #[test]
    fn classifies_memory_kind() {
        assert_eq!(candidate_kind("I prefer Tailwind CSS"), "preference");
        assert_eq!(candidate_kind("Never force push"), "instruction");
        assert_eq!(candidate_kind("For this project use pnpm"), "project");
    }

    #[test]
    fn memory_query_tokens_are_bounded() {
        let tokens = query_tokens("Remember TypeScript TypeScript workspace conventions");
        assert!(tokens.contains(&"typescript".to_string()));
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.as_str() == "typescript")
                .count(),
            1
        );
        assert!(tokens.len() <= 20);
    }
}
