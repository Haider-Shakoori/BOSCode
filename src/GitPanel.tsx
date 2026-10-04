import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { WorkspaceSummary } from "./WorkspacePanel";

type GitFileStatus = {
  path: string;
  indexStatus: string;
  worktreeStatus: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
};

type GitBranch = {
  name: string;
  current: boolean;
  upstream: string | null;
};

type GitCommit = {
  sha: string;
  subject: string;
  author: string;
  relativeTime: string;
};

type GitRemote = {
  name: string;
  fetchUrl: string;
  pushUrl: string;
};

type GitHubStatus = {
  ghInstalled: boolean;
  authenticated: boolean;
  repository: string | null;
  webUrl: string | null;
};

type GitSnapshot = {
  isRepository: boolean;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  clean: boolean;
  files: GitFileStatus[];
  branches: GitBranch[];
  commits: GitCommit[];
  remotes: GitRemote[];
  github: GitHubStatus;
};

type GitActionProposal = {
  id: string;
  kind: string;
  summary: string;
};

type GitActionResult = {
  kind: string;
  success: boolean;
  output: string;
  webUrl: string | null;
};

type GitPanelProps = {
  workspace: WorkspaceSummary | null;
  onNotice: (message: string) => void;
  onWorkspaceRefresh: () => Promise<void>;
};

const blankSnapshot: GitSnapshot = {
  isRepository: false,
  branch: null,
  upstream: null,
  ahead: 0,
  behind: 0,
  clean: true,
  files: [],
  branches: [],
  commits: [],
  remotes: [],
  github: {
    ghInstalled: false,
    authenticated: false,
    repository: null,
    webUrl: null,
  },
};

const statusLabel = (file: GitFileStatus) => {
  if (file.untracked) return "U";
  if (file.indexStatus === "A") return "A";
  if (file.indexStatus === "D" || file.worktreeStatus === "D") return "D";
  if (file.indexStatus === "R") return "R";
  return "M";
};

export default function GitPanel({
  workspace,
  onNotice,
  onWorkspaceRefresh,
}: GitPanelProps) {
  const [snapshot, setSnapshot] = useState<GitSnapshot>(blankSnapshot);
  const [proposals, setProposals] = useState<GitActionProposal[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [commitMessage, setCommitMessage] = useState("");
  const [newBranch, setNewBranch] = useState("");
  const [prTitle, setPrTitle] = useState("");
  const [prBody, setPrBody] = useState("");
  const [prDraft, setPrDraft] = useState(false);
  const [result, setResult] = useState<GitActionResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const stagedFiles = useMemo(
    () => snapshot.files.filter((file) => file.staged),
    [snapshot.files],
  );
  const unstagedFiles = useMemo(
    () => snapshot.files.filter((file) => file.unstaged),
    [snapshot.files],
  );

  const load = async () => {
    if (!workspace) {
      setSnapshot(blankSnapshot);
      setProposals([]);
      setSelected([]);
      return;
    }

    try {
      const [nextSnapshot, pending] = await Promise.all([
        invoke<GitSnapshot>("git_snapshot"),
        invoke<GitActionProposal[]>("list_git_action_proposals"),
      ]);
      setSnapshot(nextSnapshot);
      setProposals(pending);
      setSelected((current) =>
        current.filter((path) => nextSnapshot.files.some((file) => file.path === path)),
      );
    } catch (caught) {
      setError(String(caught));
    }
  };

  useEffect(() => {
    void load();
  }, [workspace?.root]);

  const propose = async <T extends GitActionProposal>(
    command: string,
    args: Record<string, unknown> = {},
  ) => {
    setBusy(true);
    setError("");
    setResult(null);

    try {
      const proposal = await invoke<T>(command, args);
      setProposals((current) => [...current, proposal]);
      onNotice(`Git action staged: ${proposal.summary}`);
      return proposal;
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to stage Git action");
      return null;
    } finally {
      setBusy(false);
    }
  };

  const execute = async (proposal: GitActionProposal) => {
    setBusy(true);
    setError("");
    setResult(null);

    try {
      const nextResult = await invoke<GitActionResult>("execute_git_action", {
        actionId: proposal.id,
      });
      setResult(nextResult);
      onNotice(`Git action completed: ${proposal.summary}`);
      await load();
      await onWorkspaceRefresh();

      if (proposal.kind === "commit") setCommitMessage("");
      if (proposal.kind === "create-branch") setNewBranch("");
    } catch (caught) {
      setError(String(caught));
      onNotice("Git action failed");
      await load();
    } finally {
      setBusy(false);
    }
  };

  const reject = async (proposal: GitActionProposal) => {
    setBusy(true);
    setError("");

    try {
      await invoke("reject_git_action", { actionId: proposal.id });
      setProposals((current) => current.filter((item) => item.id !== proposal.id));
      onNotice("Git action rejected");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const togglePath = (path: string) => {
    setSelected((current) =>
      current.includes(path)
        ? current.filter((item) => item !== path)
        : [...current, path],
    );
  };

  const fillPrDraft = () => {
    const branch = snapshot.branch ?? "feature branch";
    const latest = snapshot.commits.slice(0, 5);
    const title =
      latest[0]?.subject && !latest[0].subject.toLowerCase().startsWith("merge")
        ? latest[0].subject
        : `Update ${branch}`;

    const body = [
      "## Summary",
      "",
      ...latest.slice(0, 3).map((commit) => `- ${commit.subject}`),
      "",
      "## Validation",
      "",
      "- [ ] Review pending changes",
      "- [ ] Run project tests/build",
      "- [ ] Confirm branch is pushed",
    ].join("\n");

    setPrTitle(title);
    setPrBody(body);
    onNotice("PR draft prepared from recent repository history");
  };

  if (!workspace) {
    return (
      <div className="git-panel git-empty">
        <div>
          <span>⑂</span>
          <strong>Open a workspace</strong>
          <p>Git and GitHub workflows become available after selecting a local project.</p>
        </div>
      </div>
    );
  }

  if (!snapshot.isRepository) {
    return (
      <div className="git-panel git-empty">
        <div>
          <span>⑂</span>
          <strong>Not a Git repository</strong>
          <p>
            BOSCode found the workspace, but it has no active Git work tree. Initialize Git
            outside BOSCode before using repository workflows.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="git-panel">
      <header className="git-header">
        <div>
          <strong>{snapshot.github.repository ?? workspace.name}</strong>
          <span>
            {snapshot.branch ?? "detached HEAD"}
            {snapshot.upstream ? ` · ${snapshot.upstream}` : " · no upstream"}
          </span>
        </div>
        <div className="git-sync-state">
          <span>↑ {snapshot.ahead}</span>
          <span>↓ {snapshot.behind}</span>
          <button onClick={() => void load()} disabled={busy}>↻</button>
        </div>
      </header>

      <div className="git-grid">
        <section className="git-column">
          <div className="git-section">
            <div className="git-section-title">
              <strong>Changes</strong>
              <span>{snapshot.files.length}</span>
            </div>

            <div className="git-files">
              {snapshot.clean ? (
                <div className="git-clean">✓ Working tree clean</div>
              ) : (
                snapshot.files.map((file) => (
                  <label key={file.path} className="git-file">
                    <input
                      type="checkbox"
                      checked={selected.includes(file.path)}
                      onChange={() => togglePath(file.path)}
                      disabled={busy}
                    />
                    <span className={`git-file-state state-${statusLabel(file).toLowerCase()}`}>
                      {statusLabel(file)}
                    </span>
                    <span className="git-file-path">{file.path}</span>
                    <small>
                      {file.staged ? "staged" : ""}
                      {file.staged && file.unstaged ? " + " : ""}
                      {file.unstaged ? "working" : ""}
                    </small>
                  </label>
                ))
              )}
            </div>

            <div className="git-change-actions">
              <button
                onClick={() =>
                  void propose("propose_git_stage", { paths: selected })
                }
                disabled={busy || selected.length === 0}
              >
                Stage Selected
              </button>
              <button
                onClick={() =>
                  void propose("propose_git_unstage", { paths: selected })
                }
                disabled={
                  busy ||
                  selected.length === 0 ||
                  !selected.some((path) =>
                    stagedFiles.some((file) => file.path === path),
                  )
                }
              >
                Unstage
              </button>
            </div>
          </div>

          <div className="git-section">
            <div className="git-section-title">
              <strong>Commit</strong>
              <span>{stagedFiles.length} staged</span>
            </div>
            <textarea
              value={commitMessage}
              onChange={(event) => setCommitMessage(event.target.value)}
              placeholder="Commit message…"
              spellCheck={false}
              disabled={busy}
            />
            <button
              className="git-primary"
              onClick={() =>
                void propose("propose_git_commit", { message: commitMessage })
              }
              disabled={busy || stagedFiles.length === 0 || !commitMessage.trim()}
            >
              Review Commit
            </button>
          </div>

          <div className="git-section">
            <div className="git-section-title">
              <strong>Branches</strong>
              <span>{snapshot.branches.length}</span>
            </div>

            <div className="git-new-branch">
              <input
                value={newBranch}
                onChange={(event) => setNewBranch(event.target.value)}
                placeholder="feat/my-branch"
                spellCheck={false}
                disabled={busy}
              />
              <button
                onClick={() =>
                  void propose("propose_git_create_branch", { branch: newBranch })
                }
                disabled={busy || !newBranch.trim()}
              >
                Create
              </button>
            </div>

            <div className="git-branches">
              {snapshot.branches.slice(0, 15).map((branch) => (
                <div key={branch.name} className={branch.current ? "current" : ""}>
                  <span>{branch.current ? "●" : "○"}</span>
                  <strong>{branch.name}</strong>
                  <small>{branch.upstream ?? ""}</small>
                  {!branch.current && (
                    <button
                      onClick={() =>
                        void propose("propose_git_switch_branch", {
                          branch: branch.name,
                        })
                      }
                      disabled={busy}
                    >
                      Switch
                    </button>
                  )}
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="git-column">
          <div className="git-section">
            <div className="git-section-title">
              <strong>Sync</strong>
              <span>{snapshot.remotes.length} remote(s)</span>
            </div>
            <div className="git-sync-actions">
              <button
                onClick={() => void propose("propose_git_pull")}
                disabled={busy || !snapshot.upstream}
                title="Uses git pull --ff-only"
              >
                ↓ Review Pull
              </button>
              <button
                className="git-primary"
                onClick={() =>
                  void propose("propose_git_push", {
                    setUpstream: !snapshot.upstream,
                    remote: "origin",
                  })
                }
                disabled={busy || !snapshot.branch}
              >
                ↑ Review Push
              </button>
            </div>
            <div className="git-safety-note">
              Pull is fast-forward-only. BOSCode does not expose force push, hard reset,
              or branch deletion.
            </div>
          </div>

          <div className="git-section">
            <div className="git-section-title">
              <strong>GitHub</strong>
              <span className={snapshot.github.authenticated ? "github-ok" : ""}>
                {snapshot.github.authenticated ? "connected" : "not connected"}
              </span>
            </div>

            <div className="github-status-card">
              <div>
                <span>CLI</span>
                <strong>{snapshot.github.ghInstalled ? "gh installed" : "gh missing"}</strong>
              </div>
              <div>
                <span>Auth</span>
                <strong>
                  {snapshot.github.authenticated ? "Authenticated" : "Sign-in required"}
                </strong>
              </div>
              <div>
                <span>Repository</span>
                <strong>{snapshot.github.repository ?? "No GitHub remote detected"}</strong>
              </div>
            </div>

            <div className="pr-assistant">
              <div className="pr-heading">
                <strong>Pull request assistant</strong>
                <button onClick={fillPrDraft} disabled={busy}>Draft from history</button>
              </div>
              <input
                value={prTitle}
                onChange={(event) => setPrTitle(event.target.value)}
                placeholder="Pull request title"
                disabled={busy}
              />
              <textarea
                value={prBody}
                onChange={(event) => setPrBody(event.target.value)}
                placeholder="Summary, validation, risks…"
                disabled={busy}
              />
              <label className="pr-draft-toggle">
                <input
                  type="checkbox"
                  checked={prDraft}
                  onChange={(event) => setPrDraft(event.target.checked)}
                  disabled={busy}
                />
                Create as draft
              </label>
              <button
                className="git-primary"
                onClick={() =>
                  void propose("propose_github_pull_request", {
                    title: prTitle,
                    body: prBody,
                    draft: prDraft,
                  })
                }
                disabled={
                  busy ||
                  !snapshot.github.ghInstalled ||
                  !snapshot.github.authenticated ||
                  !prTitle.trim()
                }
              >
                Review PR Creation
              </button>
            </div>
          </div>

          <div className="git-section git-history-section">
            <div className="git-section-title">
              <strong>Recent commits</strong>
              <span>{snapshot.commits.length}</span>
            </div>
            <div className="git-history">
              {snapshot.commits.map((commit) => (
                <div key={commit.sha}>
                  <code>{commit.sha}</code>
                  <strong>{commit.subject}</strong>
                  <span>{commit.author} · {commit.relativeTime}</span>
                </div>
              ))}
            </div>
          </div>
        </section>
      </div>

      <aside className="git-approval-drawer">
        <div className="git-section-title">
          <strong>Pending Git approvals</strong>
          <span>{proposals.length}</span>
        </div>

        {proposals.length === 0 ? (
          <p>No pending Git action.</p>
        ) : (
          proposals.map((proposal) => (
            <article key={proposal.id}>
              <div>
                <span>{proposal.kind}</span>
                <strong>{proposal.summary}</strong>
              </div>
              <div>
                <button onClick={() => void reject(proposal)} disabled={busy}>
                  Reject
                </button>
                <button
                  className="git-primary"
                  onClick={() => void execute(proposal)}
                  disabled={busy}
                >
                  Execute
                </button>
              </div>
            </article>
          ))
        )}

        {error && <div className="git-error">{error}</div>}
        {result && (
          <div className="git-result">
            <strong>{result.success ? "Completed" : "Failed"}</strong>
            {result.output && <pre>{result.output}</pre>}
            {result.webUrl && <span>PR: {result.webUrl}</span>}
          </div>
        )}
      </aside>
    </div>
  );
}
