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
    transport: String,
}

#[derive(Serialize)]
struct OpenCodeStatus {
    installed: bool,
    path: Option<String>,
    version: Option<String>,
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

fn opencode_binary() -> Result<PathBuf, String> {
    which::which("opencode").map_err(|_| {
        "OpenCode CLI is not installed or is not available on PATH. Install OpenCode, then restart BOSCode.".to_string()
    })
}

fn normalize_opencode_model(model: &str) -> String {
    let model = model.trim();
    if model.contains('/') {
        model.to_string()
    } else {
        format!("opencode/{model}")
    }
}

fn opencode_runtime_config() -> String {
    json!({
        "permission": {
            "read": "deny",
            "edit": "deny",
            "glob": "deny",
            "grep": "deny",
            "list": "deny",
            "bash": "deny",
            "task": "deny",
            "external_directory": "deny",
            "todowrite": "deny",
            "webfetch": "deny",
            "websearch": "deny",
            "lsp": "deny",
            "skill": "deny",
            "question": "deny"
        }
    })
    .to_string()
}

fn opencode_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::from(
        "You are the AI model runtime for BOSCode. The conversation and repository context are supplied below. Do not use tools, edit files, or run commands. Return only the assistant response that BOSCode should display.\n\n",
    );

    for message in messages {
        let role = match message.role.as_str() {
            "system" => "SYSTEM",
            "assistant" => "ASSISTANT",
            _ => "USER",
        };
        prompt.push_str(role);
        prompt.push_str(":\n");
        prompt.push_str(&message.content);
        prompt.push_str("\n\n");
    }

    prompt.push_str("ASSISTANT:\n");
    prompt
}

fn opencode_text_event(value: &Value) -> Option<&str> {
    if value.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }

    value
        .get("part")
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str)
}

fn opencode_error_event(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }

    if let Some(message) = value
        .pointer("/error/data/message")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error/message").and_then(Value::as_str))
    {
        return Some(message.to_string());
    }

    value
        .get("error")
        .map(|error| error.to_string())
        .filter(|error| !error.is_empty())
}

async fn spawn_opencode(
    model: &str,
    prompt: &str,
    secret: &str,
) -> Result<Child, String> {
    let binary = opencode_binary()?;
    let mut command = Command::new(binary);
    command
        .arg("--pure")
        .arg("run")
        .arg("--format")
        .arg("json")
        .arg("--model")
        .arg(model)
        .arg("--agent")
        .arg("plan")
        .env("OPENCODE_API_KEY", secret)
        .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
        .env("OPENCODE_DISABLE_TERMINAL_TITLE", "true")
        .env("OPENCODE_CONFIG_CONTENT", opencode_runtime_config())
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Unable to start OpenCode CLI: {error}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Unable to open OpenCode input stream.".to_string())?;

    stdin
        .write_all(prompt.as_bytes())
        .await
        .map_err(|error| format!("Unable to send the prompt to OpenCode: {error}"))?;
    stdin
        .shutdown()
        .await
        .map_err(|error| format!("Unable to finish the OpenCode input stream: {error}"))?;

    Ok(child)
}

async fn run_opencode_collect(model: &str, prompt: &str, secret: &str) -> Result<String, String> {
    let mut child = spawn_opencode(model, prompt, secret).await?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Unable to read OpenCode output.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Unable to read OpenCode diagnostics.".to_string())?;

    let stderr_task = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut diagnostics = String::new();
        let _ = stderr.read_to_string(&mut diagnostics).await;
        diagnostics
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut answer = String::new();
    let mut event_error: Option<String> = None;

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("Unable to read OpenCode output: {error}"))?
    {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let Some(message) = opencode_error_event(&value) {
            event_error = Some(message);
            continue;
        }

        if let Some(text) = opencode_text_event(&value) {
            if !text.trim().is_empty() {
                if !answer.is_empty() {
                    answer.push_str("\n\n");
                }
                answer.push_str(text.trim());
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|error| format!("Unable to wait for OpenCode: {error}"))?;
    let diagnostics = stderr_task.await.unwrap_or_default();

    if let Some(message) = event_error {
        return Err(format!("OpenCode returned an error: {message}"));
    }

    if !status.success() {
        let diagnostics = diagnostics.trim();
        return Err(if diagnostics.is_empty() {
            format!("OpenCode exited with status {status}.")
        } else {
            format!("OpenCode failed: {}", diagnostics.chars().take(500).collect::<String>())
        });
    }

    if answer.trim().is_empty() {
        return Err("OpenCode completed without returning assistant text.".to_string());
    }

    Ok(answer)
}

#[tauri::command]
async fn opencode_status() -> Result<OpenCodeStatus, String> {
    let path = match opencode_binary() {
        Ok(path) => path,
        Err(_) => {
            return Ok(OpenCodeStatus {
                installed: false,
                path: None,
                version: None,
            })
        }
    };

    let version = Command::new(&path)
        .arg("--version")
        .output()
        .await
        .ok()
        .and_then(|output| {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!value.is_empty()).then_some(value)
        });

    Ok(OpenCodeStatus {
        installed: true,
        path: Some(path.display().to_string()),
        version,
    })
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
