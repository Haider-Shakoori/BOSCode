use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

const MAX_TITLE_CHARS: usize = 90;
const MAX_MESSAGE_CHARS: usize = 200_000;
const MAX_SESSION_MESSAGES: usize = 500;

#[derive(Default)]
pub struct HistoryState {
    connection: Mutex<Option<Connection>>,
    sequence: AtomicU64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    id: String,
    title: String,
    workspace: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    pinned: bool,
    archived: bool,
    summary: Option<String>,
    created_at: i64,
    updated_at: i64,
    last_opened_at: i64,
    message_count: i64,
    preview: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    id: String,
    role: String,
    content: String,
    created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    session: SessionSummary,
    messages: Vec<SessionMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageInput {
    id: Option<String>,
    role: String,
    content: String,
}

impl HistoryState {
    pub fn initialize(&self, app_data_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(app_data_dir)
            .map_err(|error| format!("Unable to create BOSCode data directory: {error}"))?;

        let connection = Connection::open(app_data_dir.join("boscode-history.sqlite3"))
            .map_err(|error| format!("Unable to open session history database: {error}"))?;
        initialize_schema(&connection)?;

        let mut guard = self
            .connection
            .lock()
            .map_err(|_| "Session history state is unavailable.".to_string())?;
        *guard = Some(connection);
        Ok(())
    }
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                workspace TEXT NULL,
                provider TEXT NULL,
                model TEXT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0,
                summary TEXT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_message_order
                ON session_messages(session_id, ordinal);
            CREATE INDEX IF NOT EXISTS idx_sessions_workspace
                ON sessions(workspace, archived, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_sessions_updated
                ON sessions(archived, pinned DESC, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_session
                ON session_messages(session_id, ordinal);
            ",
        )
        .map_err(|error| format!("Unable to initialize session history: {error}"))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn next_id(state: &HistoryState, prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
}

fn normalize_title(title: &str) -> String {
    let compact = title.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(MAX_TITLE_CHARS).collect()
}

fn derive_title(content: &str) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = compact
        .trim_matches(|character: char| {
            character.is_ascii_punctuation() && !matches!(character, '-' | '_' | '#')
        })
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect::<String>();

    if title.is_empty() {
        "New coding session".to_string()
    } else {
        title
    }
}

fn validate_role(role: &str) -> Result<(), String> {
    if matches!(role, "user" | "assistant" | "system") {
        Ok(())
    } else {
        Err("Unsupported session message role.".to_string())
    }
}

fn validate_messages(messages: &[SessionMessageInput]) -> Result<(), String> {
    if messages.len() > MAX_SESSION_MESSAGES {
        return Err(format!(
            "A BOSCode session can store at most {MAX_SESSION_MESSAGES} messages."
        ));
    }

    for message in messages {
        validate_role(&message.role)?;
        if message.content.chars().count() > MAX_MESSAGE_CHARS {
            return Err("A session message is too large to store.".to_string());
        }
    }

    Ok(())
}

fn with_connection<T>(
    state: &State<'_, HistoryState>,
    operation: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state
        .connection
        .lock()
        .map_err(|_| "Session history state is unavailable.".to_string())?;
    let connection = guard
        .as_ref()
        .ok_or_else(|| "Session history is not initialized.".to_string())?;
    operation(connection)
}

fn read_session_summary(connection: &Connection, id: &str) -> Result<SessionSummary, String> {
    connection
        .query_row(
            "
            SELECT
                s.id,
                s.title,
                s.workspace,
                s.provider,
                s.model,
                s.pinned,
                s.archived,
                s.summary,
                s.created_at,
                s.updated_at,
                s.last_opened_at,
                COUNT(m.id) AS message_count,
                (
                    SELECT substr(content, 1, 180)
                    FROM session_messages preview
                    WHERE preview.session_id = s.id
                    ORDER BY preview.ordinal DESC
                    LIMIT 1
                ) AS preview
            FROM sessions s
            LEFT JOIN session_messages m ON m.session_id = s.id
            WHERE s.id = ?1
            GROUP BY s.id
            ",
            params![id],
            |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    workspace: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    pinned: row.get::<_, i64>(5)? != 0,
                    archived: row.get::<_, i64>(6)? != 0,
                    summary: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    last_opened_at: row.get(10)?,
                    message_count: row.get(11)?,
                    preview: row.get(12)?,
                })
            },
        )
        .map_err(|error| format!("Unable to read coding session: {error}"))
}

#[tauri::command]
pub fn list_sessions(
    workspace: Option<String>,
    query: Option<String>,
    include_archived: Option<bool>,
    state: State<'_, HistoryState>,
) -> Result<Vec<SessionSummary>, String> {
    with_connection(&state, |connection| {
        let query = query.unwrap_or_default().trim().to_ascii_lowercase();
        let include_archived = include_archived.unwrap_or(false);

        let mut statement = connection
            .prepare(
                "
                SELECT
                    s.id,
                    s.title,
                    s.workspace,
                    s.provider,
                    s.model,
                    s.pinned,
                    s.archived,
                    s.summary,
                    s.created_at,
                    s.updated_at,
                    s.last_opened_at,
                    COUNT(m.id) AS message_count,
                    (
                        SELECT substr(content, 1, 180)
                        FROM session_messages preview
                        WHERE preview.session_id = s.id
                        ORDER BY preview.ordinal DESC
                        LIMIT 1
                    ) AS preview
                FROM sessions s
                LEFT JOIN session_messages m ON m.session_id = s.id
                WHERE (?1 = 1 OR s.archived = 0)
                  AND (?2 IS NULL OR s.workspace = ?2)
                  AND (
                        ?3 = ''
                        OR lower(s.title) LIKE '%' || ?3 || '%'
                        OR lower(COALESCE(s.summary, '')) LIKE '%' || ?3 || '%'
                        OR EXISTS (
                            SELECT 1
                            FROM session_messages search_message
                            WHERE search_message.session_id = s.id
                              AND lower(search_message.content) LIKE '%' || ?3 || '%'
                        )
                  )
                GROUP BY s.id
                ORDER BY s.pinned DESC, s.updated_at DESC
                LIMIT 250
                ",
            )
            .map_err(|error| format!("Unable to prepare session list: {error}"))?;

        let rows = statement
            .query_map(
                params![if include_archived { 1 } else { 0 }, workspace, query],
                |row| {
                    Ok(SessionSummary {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        workspace: row.get(2)?,
                        provider: row.get(3)?,
                        model: row.get(4)?,
                        pinned: row.get::<_, i64>(5)? != 0,
                        archived: row.get::<_, i64>(6)? != 0,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_opened_at: row.get(10)?,
                        message_count: row.get(11)?,
                        preview: row.get(12)?,
                    })
                },
            )
            .map_err(|error| format!("Unable to list coding sessions: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read coding sessions: {error}"))
    })
}

#[tauri::command]
pub fn create_session(
    title: Option<String>,
    workspace: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    state: State<'_, HistoryState>,
) -> Result<SessionSummary, String> {
    with_connection(&state, |connection| {
        let id = next_id(&state, "session");
        let timestamp = now();
        let title = title
            .map(|value| normalize_title(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "New coding session".to_string());

        connection
            .execute(
                "
                INSERT INTO sessions (
                    id, title, workspace, provider, model, pinned, archived,
                    summary, created_at, updated_at, last_opened_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, NULL, ?6, ?6, ?6)
                ",
                params![id, title, workspace, provider, model, timestamp],
            )
            .map_err(|error| format!("Unable to create coding session: {error}"))?;

        read_session_summary(connection, &id)
    })
}

#[tauri::command]
pub fn get_session(id: String, state: State<'_, HistoryState>) -> Result<SessionDetail, String> {
    with_connection(&state, |connection| {
        connection
            .execute(
                "UPDATE sessions SET last_opened_at = ?2 WHERE id = ?1",
                params![id, now()],
            )
            .map_err(|error| format!("Unable to open coding session: {error}"))?;

        let session = read_session_summary(connection, &id)?;
        let mut statement = connection
            .prepare(
                "
                SELECT id, role, content, created_at
                FROM session_messages
                WHERE session_id = ?1
                ORDER BY ordinal
                ",
            )
            .map_err(|error| format!("Unable to prepare session messages: {error}"))?;

        let rows = statement
            .query_map(params![id], |row| {
                Ok(SessionMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|error| format!("Unable to read session messages: {error}"))?;

        let messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode session messages: {error}"))?;

        Ok(SessionDetail { session, messages })
    })
}

#[tauri::command]
pub fn save_session_messages(
    session_id: String,
    messages: Vec<SessionMessageInput>,
    provider: Option<String>,
    model: Option<String>,
    summary: Option<String>,
    state: State<'_, HistoryState>,
) -> Result<SessionSummary, String> {
    validate_messages(&messages)?;

    let mut guard = state
        .connection
        .lock()
        .map_err(|_| "Session history state is unavailable.".to_string())?;
    let connection = guard
        .as_mut()
        .ok_or_else(|| "Session history is not initialized.".to_string())?;

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to start session save: {error}"))?;
    let timestamp = now();

    let current_title = transaction
        .query_row(
            "SELECT title FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read session title: {error}"))?
        .ok_or_else(|| "Coding session does not exist.".to_string())?;

    transaction
        .execute(
            "DELETE FROM session_messages WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(|error| format!("Unable to replace session messages: {error}"))?;

    for (ordinal, message) in messages.iter().enumerate() {
        let id = message
            .id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| next_id(&state, "message"));

        transaction
            .execute(
                "
                INSERT INTO session_messages (
                    id, session_id, ordinal, role, content, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
                params![
                    id,
                    session_id,
                    ordinal as i64,
                    message.role,
                    message.content,
                    timestamp
                ],
            )
            .map_err(|error| format!("Unable to save session message: {error}"))?;
    }

    let generated_title = if current_title == "New coding session" {
        messages
            .iter()
            .find(|message| message.role == "user" && !message.content.trim().is_empty())
            .map(|message| derive_title(&message.content))
            .unwrap_or(current_title)
    } else {
        current_title
    };

    transaction
        .execute(
            "
            UPDATE sessions
            SET title = ?2,
                provider = COALESCE(?3, provider),
                model = COALESCE(?4, model),
                summary = COALESCE(?5, summary),
                updated_at = ?6,
                last_opened_at = ?6
            WHERE id = ?1
            ",
            params![
                session_id,
                generated_title,
                provider,
                model,
                summary,
                timestamp
            ],
        )
        .map_err(|error| format!("Unable to update coding session: {error}"))?;

    transaction
        .commit()
        .map_err(|error| format!("Unable to commit coding session: {error}"))?;

    read_session_summary(connection, &session_id)
}

#[tauri::command]
pub fn build_session_context(
    id: String,
    max_chars: Option<usize>,
    state: State<'_, HistoryState>,
) -> Result<String, String> {
    let budget = max_chars.unwrap_or(24_000).clamp(2_000, 64_000);

    with_connection(&state, |connection| {
        let session = read_session_summary(connection, &id)?;

        let mut statement = connection
            .prepare(
                "
                SELECT role, content
                FROM session_messages
                WHERE session_id = ?1
                ORDER BY ordinal DESC
                LIMIT 120
                ",
            )
            .map_err(|error| format!("Unable to prepare session context: {error}"))?;

        let rows = statement
            .query_map(params![id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to read session context: {error}"))?;

        let mut recent = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode session context: {error}"))?;

        let mut output = String::new();
        output.push_str("BOSCode session history context\n");
        output.push_str(&format!("Session: {}\n", session.title));

        if let Some(workspace) = &session.workspace {
            output.push_str(&format!("Workspace: {workspace}\n"));
        }

        if let Some(summary) = &session.summary {
            output.push_str("Earlier session summary:\n");
            output.push_str(summary);
            output.push_str("\n\n");
        }

        let header_chars = output.chars().count();
        if header_chars >= budget {
            return Ok(output.chars().take(budget).collect());
        }

        let available = budget.saturating_sub(header_chars);
        let mut selected = Vec::new();
        let mut used = 0usize;

        for (role, content) in recent.drain(..) {
            let line = format!("{}: {}\n", role, content);
            let chars = line.chars().count();

            if used + chars > available && !selected.is_empty() {
                break;
            }

            used += chars;
            selected.push(line);

            if used >= available {
                break;
            }
        }

        selected.reverse();
        if !selected.is_empty() {
            output.push_str("Recent messages:\n");
            for line in selected {
                output.push_str(&line);
            }
        }

        Ok(output.chars().take(budget).collect())
    })
}

#[tauri::command]
pub fn rename_session(
    id: String,
    title: String,
    state: State<'_, HistoryState>,
) -> Result<SessionSummary, String> {
    let title = normalize_title(&title);
    if title.is_empty() {
        return Err("Session title cannot be empty.".to_string());
    }

    with_connection(&state, |connection| {
        connection
            .execute(
                "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, title, now()],
            )
            .map_err(|error| format!("Unable to rename coding session: {error}"))?;
        read_session_summary(connection, &id)
    })
}

#[tauri::command]
pub fn set_session_pinned(
    id: String,
    pinned: bool,
    state: State<'_, HistoryState>,
) -> Result<SessionSummary, String> {
    with_connection(&state, |connection| {
        connection
            .execute(
                "UPDATE sessions SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, if pinned { 1 } else { 0 }, now()],
            )
            .map_err(|error| format!("Unable to update session pin: {error}"))?;
        read_session_summary(connection, &id)
    })
}

#[tauri::command]
pub fn set_session_archived(
    id: String,
    archived: bool,
    state: State<'_, HistoryState>,
) -> Result<SessionSummary, String> {
    with_connection(&state, |connection| {
        connection
            .execute(
                "UPDATE sessions SET archived = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, if archived { 1 } else { 0 }, now()],
            )
            .map_err(|error| format!("Unable to update session archive: {error}"))?;
        read_session_summary(connection, &id)
    })
}

#[tauri::command]
pub fn delete_session(id: String, state: State<'_, HistoryState>) -> Result<bool, String> {
    with_connection(&state, |connection| {
        connection
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .map(|affected| affected > 0)
            .map_err(|error| format!("Unable to delete coding session: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{derive_title, initialize_schema, normalize_title};
    use rusqlite::Connection;

    #[test]
    fn generates_compact_session_titles() {
        assert_eq!(
            derive_title("   Fix   checkout rounding in the invoice screen   "),
            "Fix checkout rounding in the invoice screen"
        );
        assert_eq!(derive_title("..."), "New coding session");
    }

    #[test]
    fn title_length_is_bounded() {
        let title = normalize_title(&"a".repeat(200));
        assert!(title.chars().count() <= 90);
    }

    #[test]
    fn derives_title_without_unbounded_prompt_growth() {
        let title = derive_title(&("Fix the checkout flow ".repeat(20)));
        assert!(title.chars().count() <= 90);
        assert!(title.starts_with("Fix the checkout flow"));
    }

    #[test]
    fn schema_enables_cascading_session_messages() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_schema(&connection).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();

        connection
            .execute(
                "INSERT INTO sessions (
                    id, title, created_at, updated_at, last_opened_at
                ) VALUES ('s1', 'Test', 1, 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_messages (
                    id, session_id, ordinal, role, content, created_at
                ) VALUES ('m1', 's1', 0, 'user', 'hello', 1)",
                [],
            )
            .unwrap();
        connection
            .execute("DELETE FROM sessions WHERE id = 's1'", [])
            .unwrap();

        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM session_messages", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
