mod changes;
mod git;
mod memory;
mod terminal;
mod workspace;

use futures_util::StreamExt;
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{ipc::Channel, Manager};

const PROVIDER_SERVICE: &str = "BOSCode AI Providers";

#[derive(Serialize)]
struct ProviderTestResult {
    provider: String,
    model: String,
    status: u16,
    latency_ms: u64,
    message: String,
}

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ChatStreamEvent {
    Started { model: String },
    Delta { content: String },
    Completed,
}

fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    let valid = !provider_id.is_empty()
        && provider_id.len() <= 80
        && provider_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });

    if valid {
        Ok(())
    } else {
        Err("Invalid provider identifier.".to_string())
    }
}

fn provider_entry(provider_id: &str) -> Result<Entry, String> {
    validate_provider_id(provider_id)?;
    Entry::new(PROVIDER_SERVICE, provider_id)
        .map_err(|error| format!("Unable to open the secure credential store: {error}"))
}

fn read_provider_secret(provider_id: &str) -> Result<Option<String>, String> {
    match provider_entry(provider_id)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Unable to read the secure credential store: {error}"
        )),
    }
}

fn provider_error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();

    if let Some(message) = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return message.chars().take(240).collect();
    }

    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "The provider did not return an error message.".to_string()
    } else {
        compact.chars().take(240).collect()
    }
}

fn chat_endpoint(base_url: &str) -> Result<reqwest::Url, String> {
    let base_url = base_url.trim().trim_end_matches('/');

    if base_url.is_empty() {
        return Err("API Base URL is required.".to_string());
    }

    let parsed_url = reqwest::Url::parse(&format!("{base_url}/chat/completions"))
        .map_err(|_| "API Base URL is not a valid URL.".to_string())?;

    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err("API Base URL must use http or https.".to_string());
    }

    Ok(parsed_url)
}

fn validate_messages(messages: &[ChatMessage]) -> Result<(), String> {
    if messages.is_empty() {
        return Err("Conversation cannot be empty.".to_string());
    }

    if messages.len() > 200 {
        return Err("Conversation is too large for a single request.".to_string());
    }

    for message in messages {
        if !matches!(message.role.as_str(), "user" | "assistant" | "system") {
            return Err("Conversation contains an unsupported message role.".to_string());
        }

        if message.content.len() > 200_000 {
            return Err("A conversation message is too large.".to_string());
        }
    }

    Ok(())
}

#[tauri::command]
fn app_info() -> String {
    format!("BOSCode {}", env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
fn save_provider_secret(provider_id: String, secret: String) -> Result<(), String> {
    let secret = secret.trim();

    if secret.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }

    provider_entry(&provider_id)?
        .set_password(secret)
        .map_err(|error| format!("Unable to store the API key securely: {error}"))
}

#[tauri::command]
fn provider_secret_exists(provider_id: String) -> Result<bool, String> {
    Ok(read_provider_secret(&provider_id)?.is_some())
}

#[tauri::command]
fn delete_provider_secret(provider_id: String) -> Result<(), String> {
    match provider_entry(&provider_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(format!("Unable to remove the API key: {error}")),
    }
}

#[tauri::command]
async fn test_provider_connection(
    provider_id: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<ProviderTestResult, String> {
    validate_provider_id(&provider_id)?;
    let parsed_url = chat_endpoint(&base_url)?;
    let model = model.trim();

    if model.is_empty() {
        return Err("Model ID is required.".to_string());
    }

    let provided_secret = api_key
        .as_deref()
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
        .map(ToOwned::to_owned);

    let secret = match provided_secret {
        Some(secret) => Some(secret),
        None => read_provider_secret(&provider_id)?,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|error| format!("Unable to initialize the HTTP client: {error}"))?;

    let payload = json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": "Reply only with OK."
            }
        ],
        "max_tokens": 4,
        "stream": false
    });

    let mut request = client.post(parsed_url).json(&payload);
    if let Some(secret) = secret {
        request = request.bearer_auth(secret);
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;

    let status = response.status();
    let status_code = status.as_u16();
    let body = response.text().await.unwrap_or_default();
    let latency_ms = started.elapsed().as_millis() as u64;

    if !status.is_success() {
        return Err(format!(
            "Provider returned HTTP {status_code}: {}",
            provider_error_message(&body)
        ));
    }

    Ok(ProviderTestResult {
        provider: provider_id,
        model: model.to_string(),
        status: status_code,
        latency_ms,
        message: "Connection verified.".to_string(),
    })
}

#[tauri::command]
async fn stream_chat(
    provider_id: String,
    base_url: String,
    model: String,
    messages: Vec<ChatMessage>,
    on_event: Channel<ChatStreamEvent>,
) -> Result<(), String> {
    validate_provider_id(&provider_id)?;
    validate_messages(&messages)?;

    let model = model.trim();
    if model.is_empty() {
        return Err("Model ID is required.".to_string());
    }

    let endpoint = chat_endpoint(&base_url)?;
    let secret = read_provider_secret(&provider_id)?;

    let request_messages = messages
        .iter()
        .map(|message| {
            json!({
                "role": message.role,
                "content": message.content,
            })
        })
        .collect::<Vec<_>>();

    let payload = json!({
        "model": model,
        "messages": request_messages,
        "stream": true
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| format!("Unable to initialize the HTTP client: {error}"))?;

    let mut request = client.post(endpoint).json(&payload);
    if let Some(secret) = secret {
        request = request.bearer_auth(secret);
    }

    on_event
        .send(ChatStreamEvent::Started {
            model: model.to_string(),
        })
        .map_err(|error| format!("Unable to stream to the BOSCode UI: {error}"))?;

    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Provider returned HTTP {status_code}: {}",
            provider_error_message(&body)
        ));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut completed = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Provider stream failed: {error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline) = buffer.find('\n') {
            let line = buffer.drain(..=newline).collect::<String>();
            let line = line.trim();

            if !line.starts_with("data:") {
                continue;
            }

            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() {
                continue;
            }

            if data == "[DONE]" {
                completed = true;
                break;
            }

            let value = match serde_json::from_str::<Value>(data) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if let Some(content) = value
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
            {
                if !content.is_empty() {
                    on_event
                        .send(ChatStreamEvent::Delta {
                            content: content.to_string(),
                        })
                        .map_err(|error| format!("Unable to stream to the BOSCode UI: {error}"))?;
                }
            }
        }

        if completed {
            break;
        }
    }

    on_event
        .send(ChatStreamEvent::Completed)
        .map_err(|error| format!("Unable to finish the BOSCode stream: {error}"))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app.state::<memory::MemoryState>()
                .initialize(&data_dir)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .manage(memory::MemoryState::default())
        .manage(workspace::WorkspaceState::default())
        .manage(git::GitState::default())
        .manage(terminal::CommandState::default())
        .manage(changes::ChangeState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            memory::list_memories,
            memory::create_memory,
            memory::update_memory,
            memory::delete_memory,
            memory::memory_stats,
            memory::capture_memory_from_message,
            memory::build_memory_context,
            workspace::set_workspace,
            workspace::get_workspace,
            workspace::list_workspace,
            workspace::read_workspace_file,
            workspace::search_workspace,
            workspace::build_workspace_context,
            changes::get_permission_mode,
            changes::set_permission_mode,
            changes::propose_workspace_change,
            changes::list_pending_changes,
            changes::reject_workspace_change,
            changes::apply_workspace_change,
            changes::undo_last_workspace_change,
            terminal::detect_workspace_commands,
            terminal::propose_command,
            terminal::list_command_proposals,
            terminal::reject_command,
            terminal::run_approved_command,
            terminal::cancel_command,
            terminal::command_history,
            terminal::latest_command_context,
            git::git_snapshot,
            git::list_git_action_proposals,
            git::propose_git_stage,
            git::propose_git_unstage,
            git::propose_git_commit,
            git::propose_git_create_branch,
            git::propose_git_switch_branch,
            git::propose_git_pull,
            git::propose_git_push,
            git::propose_github_pull_request,
            git::reject_git_action,
            git::execute_git_action,
            save_provider_secret,
            provider_secret_exists,
            delete_provider_secret,
            test_provider_connection,
            stream_chat
        ])
        .run(tauri::generate_context!())
        .expect("error while running BOSCode");
}
