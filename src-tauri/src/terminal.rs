use crate::workspace::{workspace_root, WorkspaceState};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{ipc::Channel, State};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::oneshot,
};

const MAX_PENDING_COMMANDS: usize = 100;
const MAX_HISTORY: usize = 50;
const MAX_COMMAND_CHARS: usize = 4_000;
const MAX_CAPTURE_CHARS: usize = 32_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandProposal {
    id: String,
    command_line: String,
    executable: String,
    args: Vec<String>,
    label: String,
    reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedCommand {
    command_line: String,
    label: String,
    reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRunSummary {
    run_id: String,
    command_line: String,
    exit_code: Option<i32>,
    success: bool,
    cancelled: bool,
    output: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CommandEvent {
    Started {
        run_id: String,
        command_line: String,
    },
    Stdout {
        run_id: String,
        line: String,
    },
    Stderr {
        run_id: String,
        line: String,
    },
    Completed {
        run_id: String,
        exit_code: Option<i32>,
        success: bool,
        cancelled: bool,
    },
}

struct RunningCommand {
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
pub struct CommandState {
    pending: Mutex<HashMap<String, CommandProposal>>,
    running: Mutex<HashMap<String, RunningCommand>>,
    history: Mutex<Vec<CommandRunSummary>>,
    sequence: AtomicU64,
}

fn next_id(prefix: &str, state: &CommandState) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
}

fn parse_command_line(command_line: &str) -> Result<(String, Vec<String>), String> {
    let command_line = command_line.trim();

    if command_line.is_empty() {
        return Err("Command cannot be empty.".to_string());
    }

    if command_line.len() > MAX_COMMAND_CHARS {
        return Err("Command is too long.".to_string());
    }

    if command_line.contains('\n') || command_line.contains('\r') {
        return Err("Commands must be approved one line at a time.".to_string());
    }

    let parts = shell_words::split(command_line)
        .map_err(|_| "Unable to parse command. Check quotes and arguments.".to_string())?;

    let executable = parts
        .first()
        .cloned()
        .ok_or_else(|| "Command cannot be empty.".to_string())?;

    if executable.contains('/') || executable.contains('\\') || executable.contains(':') {
        return Err(
            "Run executables by name only. BOSCode does not approve arbitrary executable paths."
                .to_string(),
        );
    }

    let args = parts.into_iter().skip(1).collect::<Vec<_>>();
    Ok((executable, args))
}


enum ResolvedExecutable {
    Direct(PathBuf),
    #[cfg(windows)]
    WindowsBatch(PathBuf),
}

fn resolve_executable(executable: &str) -> Result<ResolvedExecutable, String> {
    let path = which::which(executable)
        .map_err(|_| format!("Executable not found on PATH: {executable}"))?;

    #[cfg(windows)]
    {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if matches!(extension.as_str(), "cmd" | "bat") {
            return Ok(ResolvedExecutable::WindowsBatch(path));
        }
    }

    Ok(ResolvedExecutable::Direct(path))
}

#[cfg(windows)]
fn safe_batch_token(value: &str) -> Result<String, String> {
    if value.chars().any(|character| {
        matches!(
            character,
            '"' | '%' | '&' | '|' | '<' | '>' | '^' | '\r' | '\n'
        )
    }) {
        return Err(
            "This argument contains shell-control characters that BOSCode will not pass to a Windows .cmd/.bat shim."
                .to_string(),
        );
    }

    Ok(format!("\"{value}\""))
}

#[cfg(windows)]
fn windows_batch_command(path: &Path, args: &[String]) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "The command path is not valid Unicode.".to_string())?;

    let mut parts = vec![safe_batch_token(path)?];
    for arg in args {
        parts.push(safe_batch_token(arg)?);
    }

    Ok(parts.join(" "))
}

fn command_line(executable: &str, args: &[String]) -> String {
    std::iter::once(executable)
        .chain(args.iter().map(String::as_str))
        .map(shell_words::quote)
        .map(|part| part.into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_capture(buffer: &mut String, prefix: &str, line: &str) {
    if buffer.len() >= MAX_CAPTURE_CHARS {
        return;
    }

    let remaining = MAX_CAPTURE_CHARS - buffer.len();
    let entry = format!("{prefix}{line}\n");
    buffer.extend(entry.chars().take(remaining));
}

fn push_history(state: &State<'_, CommandState>, summary: CommandRunSummary) -> Result<(), String> {
    let mut history = state
        .history
        .lock()
        .map_err(|_| "Command history is unavailable.".to_string())?;
    history.push(summary);
    if history.len() > MAX_HISTORY {
        history.remove(0);
    }
    Ok(())
}

fn package_scripts(root: &std::path::Path) -> Vec<RecommendedCommand> {
    let package_path = root.join("package.json");
    let Ok(content) = fs::read_to_string(package_path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let Some(scripts) = value.get("scripts").and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut commands = Vec::new();

    for (script, label, reason) in [
        ("typecheck", "Typecheck", "Validate TypeScript types"),
        ("lint", "Lint", "Run the project's lint checks"),
        ("test", "Test", "Run the project's automated tests"),
        ("build", "Build", "Compile the project"),
    ] {
        if scripts.contains_key(script) {
            commands.push(RecommendedCommand {
                command_line: format!("npm run {script}"),
                label: label.to_string(),
                reason: reason.to_string(),
            });
        }
    }

    commands
}

#[tauri::command]
pub fn detect_workspace_commands(
    workspace: State<'_, WorkspaceState>,
) -> Result<Vec<RecommendedCommand>, String> {
    let root = workspace_root(&workspace)?;
    let mut commands = package_scripts(&root);

    if root.join("artisan").is_file() {
        commands.push(RecommendedCommand {
            command_line: "php artisan test".to_string(),
            label: "Laravel tests".to_string(),
            reason: "Run the Laravel application test suite".to_string(),
        });
    } else if root.join("composer.json").is_file() {
        commands.push(RecommendedCommand {
            command_line: "composer test".to_string(),
            label: "Composer tests".to_string(),
            reason: "Run the Composer project test script if configured".to_string(),
        });
    }

    if root.join("Cargo.toml").is_file() {
        commands.push(RecommendedCommand {
            command_line: "cargo test".to_string(),
            label: "Rust tests".to_string(),
            reason: "Run the Rust test suite".to_string(),
        });
        commands.push(RecommendedCommand {
            command_line: "cargo check".to_string(),
            label: "Rust check".to_string(),
            reason: "Compile-check the Rust project without producing release artifacts".to_string(),
        });
    }

    if root.join("pubspec.yaml").is_file() {
        commands.push(RecommendedCommand {
            command_line: "flutter test".to_string(),
            label: "Flutter tests".to_string(),
            reason: "Run the Flutter test suite".to_string(),
        });
        commands.push(RecommendedCommand {
            command_line: "flutter analyze".to_string(),
            label: "Flutter analyze".to_string(),
            reason: "Run static analysis for the Flutter project".to_string(),
        });
    }

    if root.join("pyproject.toml").is_file() || root.join("pytest.ini").is_file() {
        commands.push(RecommendedCommand {
            command_line: "pytest".to_string(),
            label: "Python tests".to_string(),
            reason: "Run the Python test suite".to_string(),
        });
    }

    commands.sort_by(|left, right| left.command_line.cmp(&right.command_line));
    commands.dedup_by(|left, right| left.command_line == right.command_line);
    Ok(commands)
}

#[tauri::command]
pub fn propose_command(
    command_line: String,
    label: Option<String>,
    reason: Option<String>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, CommandState>,
) -> Result<CommandProposal, String> {
    let _ = workspace_root(&workspace)?;
    let (executable, args) = parse_command_line(&command_line)?;

    let proposal = CommandProposal {
        id: next_id("cmd", &state),
        command_line: command_line(&executable, &args),
        executable,
        args,
        label: label
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Run command".to_string()),
        reason: reason
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Requested workspace command".to_string()),
    };

    let mut pending = state
        .pending
        .lock()
        .map_err(|_| "Command approval state is unavailable.".to_string())?;

    if pending.len() >= MAX_PENDING_COMMANDS {
        return Err("Too many pending commands. Run or reject some commands first.".to_string());
    }

    pending.insert(proposal.id.clone(), proposal.clone());
    Ok(proposal)
}

#[tauri::command]
pub fn list_command_proposals(
    state: State<'_, CommandState>,
) -> Result<Vec<CommandProposal>, String> {
    let pending = state
        .pending
        .lock()
        .map_err(|_| "Command approval state is unavailable.".to_string())?;
    let mut proposals = pending.values().cloned().collect::<Vec<_>>();
    proposals.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(proposals)
}

#[tauri::command]
pub fn reject_command(
    command_id: String,
    state: State<'_, CommandState>,
) -> Result<bool, String> {
    Ok(state
        .pending
        .lock()
        .map_err(|_| "Command approval state is unavailable.".to_string())?
        .remove(&command_id)
        .is_some())
}

#[tauri::command]
pub fn command_history(
    state: State<'_, CommandState>,
) -> Result<Vec<CommandRunSummary>, String> {
    Ok(state
        .history
        .lock()
        .map_err(|_| "Command history is unavailable.".to_string())?
        .clone())
}

#[tauri::command]
pub fn latest_command_context(
    state: State<'_, CommandState>,
) -> Result<Option<String>, String> {
    let history = state
        .history
        .lock()
        .map_err(|_| "Command history is unavailable.".to_string())?;

    let Some(last) = history.last() else {
        return Ok(None);
    };

    let status = if last.cancelled {
        "cancelled".to_string()
    } else if let Some(code) = last.exit_code {
        format!("exit code {code}")
    } else {
        "no exit code".to_string()
    };

    Ok(Some(format!(
        "Latest approved BOSCode command\nCommand: {}\nStatus: {}\nSuccess: {}\nOutput:\n{}",
        last.command_line, status, last.success, last.output
    )))
}

#[tauri::command]
pub async fn run_approved_command(
    command_id: String,
    on_event: Channel<CommandEvent>,
    workspace: State<'_, WorkspaceState>,
    state: State<'_, CommandState>,
) -> Result<CommandRunSummary, String> {
    let root = workspace_root(&workspace)?;

    let proposal = state
        .pending
        .lock()
        .map_err(|_| "Command approval state is unavailable.".to_string())?
        .remove(&command_id)
        .ok_or_else(|| "This command is not pending approval.".to_string())?;

    let run_id = next_id("run", &state);
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();

    state
        .running
        .lock()
        .map_err(|_| "Running command state is unavailable.".to_string())?
        .insert(
            run_id.clone(),
            RunningCommand {
                cancel: Some(cancel_tx),
            },
        );

    let resolved = resolve_executable(&proposal.executable)?;
    let mut command = match resolved {
        ResolvedExecutable::Direct(path) => {
            let mut command = Command::new(path);
            command.args(&proposal.args);
            command
        }
        #[cfg(windows)]
        ResolvedExecutable::WindowsBatch(path) => {
            let mut command = Command::new("cmd.exe");
            let wrapped = windows_batch_command(&path, &proposal.args)?;
            command.args(["/D", "/V:OFF", "/S", "/C", &wrapped]);
            command
        }
    };

    command
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        let _ = state
            .running
            .lock()
            .map(|mut running| running.remove(&run_id));
        format!("Unable to start {}: {error}", proposal.executable)
    })?;

    on_event
        .send(CommandEvent::Started {
            run_id: run_id.clone(),
            command_line: proposal.command_line.clone(),
        })
        .map_err(|error| format!("Unable to stream command state to the UI: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Unable to capture command stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Unable to capture command stderr.".to_string())?;

    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();

    let stdout_tx = line_tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = stdout_tx.send((false, line));
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = line_tx.send((true, line));
        }
    });

    let mut captured = String::new();
    let mut cancelled = false;
    let status = loop {
        tokio::select! {
            line = line_rx.recv() => {
                match line {
                    Some((is_stderr, line)) => {
                        push_capture(&mut captured, if is_stderr { "[stderr] " } else { "" }, &line);
                        let event = if is_stderr {
                            CommandEvent::Stderr { run_id: run_id.clone(), line }
                        } else {
                            CommandEvent::Stdout { run_id: run_id.clone(), line }
                        };
                        let _ = on_event.send(event);
                    }
                    None => {
                        match child.wait().await {
                            Ok(status) => break Some(status),
                            Err(error) => {
                                state.running.lock().ok().map(|mut running| running.remove(&run_id));
                                return Err(format!("Unable to wait for command: {error}"));
                            }
                        }
                    }
                }
            }
            result = child.wait() => {
                match result {
                    Ok(status) => break Some(status),
                    Err(error) => {
                        state.running.lock().ok().map(|mut running| running.remove(&run_id));
                        return Err(format!("Unable to wait for command: {error}"));
                    }
                }
            }
            _ = &mut cancel_rx => {
                cancelled = true;
                let _ = child.kill().await;
                let waited = child.wait().await.ok();
                break waited;
            }
        }
    };

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    while let Ok((is_stderr, line)) = line_rx.try_recv() {
        push_capture(&mut captured, if is_stderr { "[stderr] " } else { "" }, &line);
        let event = if is_stderr {
            CommandEvent::Stderr { run_id: run_id.clone(), line }
        } else {
            CommandEvent::Stdout { run_id: run_id.clone(), line }
        };
        let _ = on_event.send(event);
    }

    let exit_code = status.and_then(|status| status.code());
    let success = !cancelled && status.map(|status| status.success()).unwrap_or(false);

    state
        .running
        .lock()
        .map_err(|_| "Running command state is unavailable.".to_string())?
        .remove(&run_id);

    let summary = CommandRunSummary {
        run_id: run_id.clone(),
        command_line: proposal.command_line,
        exit_code,
        success,
        cancelled,
        output: captured,
    };

    push_history(&state, summary.clone())?;

    let _ = on_event.send(CommandEvent::Completed {
        run_id,
        exit_code,
        success,
        cancelled,
    });

    Ok(summary)
}

#[tauri::command]
pub fn cancel_command(
    run_id: String,
    state: State<'_, CommandState>,
) -> Result<bool, String> {
    let mut running = state
        .running
        .lock()
        .map_err(|_| "Running command state is unavailable.".to_string())?;

    let Some(command) = running.get_mut(&run_id) else {
        return Ok(false);
    };

    if let Some(cancel) = command.cancel.take() {
        let _ = cancel.send(());
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{command_line, parse_command_line};

    #[test]
    fn parses_arguments_without_invoking_a_shell() {
        let (executable, args) = parse_command_line("npm run test -- --watch=false").unwrap();
        assert_eq!(executable, "npm");
        assert_eq!(args, vec!["run", "test", "--", "--watch=false"]);
    }

    #[test]
    fn shell_metacharacters_are_inert_arguments() {
        let (executable, args) = parse_command_line("npm test && whoami").unwrap();
        assert_eq!(executable, "npm");
        assert!(args.contains(&"&&".to_string()));
        assert!(args.contains(&"whoami".to_string()));
    }

    #[test]
    fn rejects_arbitrary_executable_paths() {
        assert!(parse_command_line("../tool.exe --run").is_err());
        assert!(parse_command_line("C:\\Windows\\System32\\cmd.exe /c whoami").is_err());
    }

    #[test]
    fn renders_canonical_command_line() {
        assert_eq!(
            command_line("npm", &["run".into(), "build".into()]),
            "npm run build"
        );
    }
}
