import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";

export type WorkspaceSummary = {
  root: string;
  name: string;
  branch: string | null;
  fileCount: number;
  directoryCount: number;
  truncated: boolean;
};

export type WorkspaceEntry = {
  path: string;
  name: string;
  isDir: boolean;
  depth: number;
  size: number | null;
};

export type WorkspaceFile = {
  path: string;
  content: string;
  size: number;
  language: string;
};

type SearchHit = {
  path: string;
  line: number;
  preview: string;
};

type WorkspacePanelProps = {
  workspace: WorkspaceSummary | null;
  entries: WorkspaceEntry[];
  activeFile: WorkspaceFile | null;
  onWorkspaceChange: (workspace: WorkspaceSummary, entries: WorkspaceEntry[]) => void;
  onFileOpen: (file: WorkspaceFile) => void;
  onNotice: (message: string) => void;
  onChangeProposed: () => void;
  mode: "files" | "search" | null;
};

const formatSize = (bytes: number | null) => {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

export default function WorkspacePanel({
  workspace,
  entries,
  activeFile,
  onWorkspaceChange,
  onFileOpen,
  onNotice,
  onChangeProposed,
  mode,
}: WorkspacePanelProps) {
  const [tab, setTab] = useState<"files" | "search" | "preview" | "new">("files");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [busy, setBusy] = useState<"open" | "search" | "file" | "propose" | null>(null);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [newPath, setNewPath] = useState("");
  const [newContent, setNewContent] = useState("");

  const visibleEntries = useMemo(() => entries.slice(0, 2500), [entries]);

  useEffect(() => {
    if (mode) setTab(mode);
  }, [mode]);

  useEffect(() => {
    setEditing(false);
    setDraft(activeFile?.content ?? "");
  }, [activeFile?.path, activeFile?.content]);

  const chooseWorkspace = async () => {
    setBusy("open");
    setError("");

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open BOSCode Workspace",
      });

      if (!selected || Array.isArray(selected)) return;

      const summary = await invoke<WorkspaceSummary>("set_workspace", { path: selected });
      const scanned = await invoke<WorkspaceEntry[]>("list_workspace");
      localStorage.setItem("boscode.workspace.path", selected);
      onWorkspaceChange(summary, scanned);
      setHits([]);
      setQuery("");
      setTab("files");
      onNotice(`Workspace opened: ${summary.name}`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to open workspace");
    } finally {
      setBusy(null);
    }
  };

  const openFile = async (path: string) => {
    setBusy("file");
    setError("");

    try {
      const file = await invoke<WorkspaceFile>("read_workspace_file", { path });
      onFileOpen(file);
      setTab("preview");
      onNotice(`Opened ${file.path}`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to open file");
    } finally {
      setBusy(null);
    }
  };

  const runSearch = async () => {
    const value = query.trim();
    if (value.length < 2 || !workspace) return;

    setBusy("search");
    setError("");

    try {
      const results = await invoke<SearchHit[]>("search_workspace", {
        query: value,
        maxResults: 100,
      });
      setHits(results);
      onNotice(`${results.length} search results for “${value}”`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Workspace search failed");
    } finally {
      setBusy(null);
    }
  };

  const proposeEdit = async () => {
    if (!activeFile || draft === activeFile.content) return;
    setBusy("propose");
    setError("");

    try {
      await invoke("propose_workspace_change", {
        path: activeFile.path,
        proposedContent: draft,
        deleteFile: false,
      });
      setEditing(false);
      onChangeProposed();
      onNotice(`Proposed change for ${activeFile.path}`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to create change proposal");
    } finally {
      setBusy(null);
    }
  };

  const proposeDelete = async () => {
    if (!activeFile) return;
    setBusy("propose");
    setError("");

    try {
      await invoke("propose_workspace_change", {
        path: activeFile.path,
        proposedContent: null,
        deleteFile: true,
      });
      onChangeProposed();
      onNotice(`Proposed deletion of ${activeFile.path}`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to create deletion proposal");
    } finally {
      setBusy(null);
    }
  };

  const proposeNewFile = async () => {
    const path = newPath.trim();
    if (!path || !workspace) return;
    setBusy("propose");
    setError("");

    try {
      await invoke("propose_workspace_change", {
        path,
        proposedContent: newContent,
        deleteFile: false,
      });
      setNewPath("");
      setNewContent("");
      setTab("files");
      onChangeProposed();
      onNotice(`Proposed new file: ${path}`);
    } catch (caught) {
      setError(String(caught));
      onNotice("Unable to create file proposal");
    } finally {
      setBusy(null);
    }
  };

  if (!workspace) {
    return (
      <aside className="right-panel workspace-empty-panel">
        <div className="workspace-empty-card">
          <div className="workspace-empty-icon">▱</div>
          <h3>Open a codebase</h3>
          <p>
            Select a local project folder. BOSCode will index its safe text files for
            navigation, search, AI context, and reviewed change proposals.
          </p>
          <button onClick={() => void chooseWorkspace()} disabled={busy !== null}>
            {busy === "open" ? "Opening…" : "Open Folder"}
          </button>
          <small>Read-only by default · sensitive files are excluded</small>
          {error && <div className="workspace-error">{error}</div>}
        </div>
      </aside>
    );
  }

  return (
    <aside className="right-panel workspace-panel">
      <div className="workspace-panel-header">
        <div className="workspace-identity">
          <span className="workspace-folder-icon">▱</span>
          <div>
            <strong>{workspace.name}</strong>
            <span title={workspace.root}>{workspace.root}</span>
          </div>
        </div>
        <div className="workspace-header-actions">
          <button
            className="workspace-switch"
            onClick={() => setTab("new")}
            disabled={busy !== null}
            title="Propose a new file"
          >
            ＋
          </button>
          <button
            className="workspace-switch"
            onClick={() => void chooseWorkspace()}
            disabled={busy !== null}
            title="Open another folder"
          >
            ⋯
          </button>
        </div>
      </div>

      <div className="workspace-meta">
        <span>⑂ {workspace.branch ?? "no git branch"}</span>
        <span>{workspace.fileCount} files</span>
        <span>{workspace.directoryCount} folders</span>
        {workspace.truncated && <span className="workspace-warning">index capped</span>}
      </div>

      <div className="workspace-tabs">
        <button className={tab === "files" ? "active" : ""} onClick={() => setTab("files")}>
          Files
        </button>
        <button className={tab === "search" ? "active" : ""} onClick={() => setTab("search")}>
          Search
        </button>
        <button
          className={tab === "preview" ? "active" : ""}
          onClick={() => setTab("preview")}
          disabled={!activeFile}
        >
          Preview
        </button>
        <button className={tab === "new" ? "active" : ""} onClick={() => setTab("new")}>
          New File
        </button>
      </div>

      {tab === "files" && (
        <div className="workspace-files">
          {visibleEntries.map((entry) => (
            <button
              key={entry.path}
              className={`workspace-entry ${entry.isDir ? "folder" : "file"} ${activeFile?.path === entry.path ? "active" : ""}`}
              style={{ paddingLeft: `${10 + Math.min(entry.depth, 10) * 13}px` }}
              onClick={() => {
                if (!entry.isDir) void openFile(entry.path);
              }}
              disabled={busy === "file" && !entry.isDir}
              title={entry.path}
            >
              <span className="workspace-entry-icon">{entry.isDir ? "▸" : "·"}</span>
              <span className="workspace-entry-name">{entry.name}</span>
              {!entry.isDir && <small>{formatSize(entry.size)}</small>}
            </button>
          ))}
          {entries.length > visibleEntries.length && (
            <div className="workspace-list-note">
              Showing the first {visibleEntries.length.toLocaleString()} indexed entries.
            </div>
          )}
        </div>
      )}

      {tab === "search" && (
        <div className="workspace-search">
          <div className="workspace-search-box">
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void runSearch();
              }}
              placeholder="Search repository text…"
              spellCheck={false}
            />
            <button
              onClick={() => void runSearch()}
              disabled={busy !== null || query.trim().length < 2}
            >
              {busy === "search" ? "…" : "⌕"}
            </button>
          </div>

          <div className="workspace-search-results">
            {hits.length === 0 ? (
              <div className="workspace-search-empty">
                Search file contents across the current workspace.
              </div>
            ) : (
              hits.map((hit, index) => (
                <button
                  key={`${hit.path}:${hit.line}:${index}`}
                  onClick={() => void openFile(hit.path)}
                >
                  <strong>{hit.path}</strong>
                  <span>Line {hit.line}</span>
                  <p>{hit.preview || "(blank line)"}</p>
                </button>
              ))
            )}
          </div>
        </div>
      )}

      {tab === "preview" && activeFile && (
        <div className="workspace-preview">
          <div className="workspace-preview-header editable">
            <div>
              <strong>{activeFile.path}</strong>
              <span>{activeFile.language} · {formatSize(activeFile.size)}</span>
            </div>
            <div className="preview-actions">
              <span className="context-badge">AI context</span>
              <button onClick={() => setEditing((value) => !value)}>
                {editing ? "Cancel" : "Edit"}
              </button>
              <button className="danger-link" onClick={() => void proposeDelete()} disabled={busy !== null}>
                Delete
              </button>
            </div>
          </div>

          {editing ? (
            <div className="workspace-editor">
              <textarea
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                spellCheck={false}
                aria-label={`Edit ${activeFile.path}`}
              />
              <div className="workspace-editor-actions">
                <span>Nothing is written until the proposal is applied from Changes.</span>
                <button
                  className="primary"
                  onClick={() => void proposeEdit()}
                  disabled={busy !== null || draft === activeFile.content}
                >
                  {busy === "propose" ? "Proposing…" : "Propose Change"}
                </button>
              </div>
            </div>
          ) : (
            <pre>
              <code>{activeFile.content}</code>
            </pre>
          )}
        </div>
      )}

      {tab === "new" && (
        <div className="new-file-editor">
          <div className="new-file-heading">
            <strong>Propose new file</strong>
            <span>The parent folder must already exist.</span>
          </div>
          <label>
            <span>Workspace-relative path</span>
            <input
              value={newPath}
              onChange={(event) => setNewPath(event.target.value)}
              placeholder="src/new-file.ts"
              spellCheck={false}
            />
          </label>
          <label className="new-file-content">
            <span>File content</span>
            <textarea
              value={newContent}
              onChange={(event) => setNewContent(event.target.value)}
              placeholder="Enter the complete file content…"
              spellCheck={false}
            />
          </label>
          <div className="workspace-editor-actions">
            <span>Secrets and paths outside this workspace are blocked.</span>
            <button
              className="primary"
              onClick={() => void proposeNewFile()}
              disabled={busy !== null || !newPath.trim()}
            >
              {busy === "propose" ? "Proposing…" : "Propose File"}
            </button>
          </div>
        </div>
      )}

      {error && <div className="workspace-error sticky">{error}</div>}
    </aside>
  );
}
