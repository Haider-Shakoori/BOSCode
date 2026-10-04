import { Channel, invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type { WorkspaceSummary } from "./WorkspacePanel";

type TerminalPanelProps = {
  workspace: WorkspaceSummary | null;
  onNotice: (message: string) => void;
};

type CommandProposal = {
  id: string;
  commandLine: string;
  executable: string;
  args: string[];
  label: string;
  reason: string;
};

type RecommendedCommand = {
  commandLine: string;
  label: string;
  reason: string;
};

type CommandRunSummary = {
  runId: string;
  commandLine: string;
  exitCode: number | null;
  success: boolean;
  cancelled: boolean;
  output: string;
};

type CommandEvent =
  | { type: "started"; runId: string; commandLine: string }
  | { type: "stdout"; runId: string; line: string }
  | { type: "stderr"; runId: string; line: string }
  | {
      type: "completed";
      runId: string;
      exitCode: number | null;
      success: boolean;
      cancelled: boolean;
    };

type OutputLine = {
  id: number;
  kind: "command" | "stdout" | "stderr" | "system";
  text: string;
};

const MAX_VISIBLE_LINES = 700;

export default function TerminalPanel({ workspace, onNotice }: TerminalPanelProps) {
  const [command, setCommand] = useState("");
  const [proposals, setProposals] = useState<CommandProposal[]>([]);
  const [recommended, setRecommended] = useState<RecommendedCommand[]>([]);
  const [history, setHistory] = useState<CommandRunSummary[]>([]);
  const [output, setOutput] = useState<OutputLine[]>([]);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [runningCommand, setRunningCommand] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const lineSequence = useRef(0);
  const outputRef = useRef<HTMLDivElement | null>(null);

  const latest = useMemo(() => history.at(-1) ?? null, [history]);

  const appendLine = (kind: OutputLine["kind"], text: string) => {
    setOutput((current) => {
      const next = [
        ...current,
        {
          id: ++lineSequence.current,
          kind,
          text,
        },
      ];
      return next.length > MAX_VISIBLE_LINES ? next.slice(-MAX_VISIBLE_LINES) : next;
    });
  };

  const loadTerminalState = async () => {
    if (!workspace) {
      setProposals([]);
      setRecommended([]);
      setHistory([]);
      setOutput([]);
      return;
    }

    try {
      const [pending, suggestions, runs] = await Promise.all([
        invoke<CommandProposal[]>("list_command_proposals"),
        invoke<RecommendedCommand[]>("detect_workspace_commands"),
        invoke<CommandRunSummary[]>("command_history"),
      ]);
      setProposals(pending);
      setRecommended(suggestions);
      setHistory(runs);
    } catch (caught) {
      setError(String(caught));
    }
  };

  useEffect(() => {
    void loadTerminalState();
  }, [workspace?.root]);

  useEffect(() => {
    outputRef.current?.scrollTo({
      top: outputRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [output]);

  const propose = async (
    commandLine: string,
    label?: string,
    reason?: string,
  ) => {
    if (!workspace || !commandLine.trim()) return;

    setBusy(true);
    setError("");

    try {
      const proposal = await invoke<CommandProposal>("propose_command", {
        commandLine: commandLine.trim(),
        label: label ?? null,
        reason: reason ?? null,
      });
      setProposals((current) => [...current, proposal]);
      setCommand("");
      appendLine("system", `Pending approval: ${proposal.commandLine}`);
      onNotice("Command staged for explicit approval");
    } catch (caught) {
      setError(String(caught));
      onNotice("Command could not be staged");
    } finally {
      setBusy(false);
    }
  };

  const reject = async (proposal: CommandProposal) => {
    setBusy(true);
    setError("");

    try {
      await invoke("reject_command", { commandId: proposal.id });
      setProposals((current) =>
        current.filter((item) => item.id !== proposal.id),
      );
      appendLine("system", `Rejected: ${proposal.commandLine}`);
      onNotice("Command rejected");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const run = async (proposal: CommandProposal) => {
    if (runningId) return;

    setBusy(true);
    setError("");
    setRunningCommand(proposal.commandLine);
    appendLine("command", `> ${proposal.commandLine}`);

    const onEvent = new Channel<CommandEvent>();
    onEvent.onmessage = (event) => {
      if (event.type === "started") {
        setRunningId(event.runId);
        setRunningCommand(event.commandLine);
        onNotice(`Running: ${event.commandLine}`);
        return;
      }

      if (event.type === "stdout") {
        appendLine("stdout", event.line);
        return;
      }

      if (event.type === "stderr") {
        appendLine("stderr", event.line);
        return;
      }

      if (event.type === "completed") {
        setRunningId(null);
        setRunningCommand("");
        const status = event.cancelled
          ? "cancelled"
          : event.success
            ? "passed"
            : `failed (exit ${event.exitCode ?? "unknown"})`;
        appendLine("system", `Command ${status}`);
      }
    };

    try {
      const summary = await invoke<CommandRunSummary>("run_approved_command", {
        commandId: proposal.id,
        onEvent,
      });
      setHistory((current) => [...current, summary]);
      setProposals((current) =>
        current.filter((item) => item.id !== proposal.id),
      );
      onNotice(
        summary.cancelled
          ? "Command cancelled"
          : summary.success
            ? "Command completed successfully"
            : `Command failed with exit code ${summary.exitCode ?? "unknown"}`,
      );
    } catch (caught) {
      setError(String(caught));
      appendLine("stderr", String(caught));
      onNotice("Command execution failed");
    } finally {
      setRunningId(null);
      setRunningCommand("");
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (!runningId) return;

    try {
      const requested = await invoke<boolean>("cancel_command", {
        runId: runningId,
      });
      if (requested) {
        appendLine("system", "Cancellation requested…");
        onNotice("Cancelling command…");
      }
    } catch (caught) {
      setError(String(caught));
    }
  };

  const clearOutput = () => {
    setOutput([]);
    onNotice("Terminal output cleared");
  };

  if (!workspace) {
    return (
      <section className="terminal-panel terminal-empty">
        <div>
          <strong>Terminal</strong>
          <span>Open a workspace to run approved project commands.</span>
        </div>
      </section>
    );
  }

  return (
    <section className="terminal-panel real-terminal">
      <div className="panel-toolbar">
        <div className="terminal-tabs">
          <button className="active">Terminal</button>
          <span className="terminal-workspace">{workspace.name}</span>
        </div>
        <div className="terminal-toolbar-actions">
          {runningId && (
            <button className="terminal-stop" onClick={() => void cancel()}>
              ■ Stop
            </button>
          )}
          <button onClick={clearOutput}>Clear</button>
        </div>
      </div>

      <div className="terminal-runtime">
        <div className="terminal-main">
          <div className="terminal-output" ref={outputRef}>
            {output.length === 0 ? (
              <div className="terminal-placeholder">
                <strong>BOSCode Terminal</strong>
                <span>
                  Commands are staged first and require an explicit Run approval.
                  No shell chaining is executed implicitly.
                </span>
              </div>
            ) : (
              output.map((line) => (
                <div className={`terminal-line ${line.kind}`} key={line.id}>
                  {line.text || " "}
                </div>
              ))
            )}
          </div>

          <div className="terminal-command-box">
            <span className="terminal-prompt">›</span>
            <input
              value={command}
              onChange={(event) => setCommand(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && command.trim() && !busy) {
                  void propose(command);
                }
              }}
              placeholder={
                runningId
                  ? `Running ${runningCommand}`
                  : "Enter a command to stage for approval…"
              }
              disabled={busy || Boolean(runningId)}
              spellCheck={false}
            />
            <button
              onClick={() => void propose(command)}
              disabled={busy || Boolean(runningId) || !command.trim()}
            >
              Stage
            </button>
          </div>

          {error && <div className="terminal-error">{error}</div>}
        </div>

        <aside className="terminal-side">
          <div className="terminal-side-section">
            <div className="terminal-side-title">
              <strong>Pending approval</strong>
              <span>{proposals.length}</span>
            </div>
            <div className="terminal-proposals">
              {proposals.length === 0 ? (
                <p>No staged commands.</p>
              ) : (
                proposals.map((proposal) => (
                  <article key={proposal.id}>
                    <code>{proposal.commandLine}</code>
                    <span>{proposal.reason}</span>
                    <div>
                      <button
                        onClick={() => void reject(proposal)}
                        disabled={busy || Boolean(runningId)}
                      >
                        Reject
                      </button>
                      <button
                        className="primary"
                        onClick={() => void run(proposal)}
                        disabled={busy || Boolean(runningId)}
                      >
                        Run
                      </button>
                    </div>
                  </article>
                ))
              )}
            </div>
          </div>

          <div className="terminal-side-section recommendations">
            <div className="terminal-side-title">
              <strong>Project checks</strong>
              <span>{recommended.length}</span>
            </div>
            <div className="terminal-recommendations">
              {recommended.length === 0 ? (
                <p>No known checks detected.</p>
              ) : (
                recommended.map((item) => (
                  <button
                    key={item.commandLine}
                    onClick={() =>
                      void propose(item.commandLine, item.label, item.reason)
                    }
                    disabled={busy || Boolean(runningId)}
                    title={item.reason}
                  >
                    <span>{item.label}</span>
                    <code>{item.commandLine}</code>
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="terminal-side-section terminal-last-run">
            <div className="terminal-side-title">
              <strong>Last run</strong>
              <span>{history.length}</span>
            </div>
            {latest ? (
              <div className={latest.success ? "run-success" : "run-failure"}>
                <strong>{latest.success ? "Passed" : latest.cancelled ? "Cancelled" : "Failed"}</strong>
                <code>{latest.commandLine}</code>
                <span>
                  {latest.exitCode === null ? "No exit code" : `Exit ${latest.exitCode}`}
                </span>
              </div>
            ) : (
              <p>No completed command yet.</p>
            )}
          </div>
        </aside>
      </div>
    </section>
  );
}
