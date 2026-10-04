import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useMemo, useState } from "react";

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
}: WorkspacePanelProps) {
  const [tab, setTab] = useState<"files" | "search" | "preview">("files");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [busy, setBusy] = useState<"open" | "search" | "file" | null>(null);
  const [error, setError] = useState("");

  const visibleEntries = useMemo(() => entries.slice(0, 2500), [entries]);

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

      const summary = await invoke<WorkspaceSummary>("set_workspace", {
        path: selected,
      });
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

  if (!workspace) {
    return (
      <aside className="right-panel workspace-empty-panel">
        <div className="workspace-empty-card">
          <div className="workspace-empty-icon">▱</div>
          <h3>Open a codebase</h3>
          <p>
            Select a local project folder. BOSCode will index its safe text files for
            navigation, search, and AI context.
          </p>
          <button onClick={() => void chooseWorkspace()} disabled={busy !== null}>
            {busy === "open" ? "Opening…" : "Open Folder"}
          </button>
          <small>Read-only in this batch · sensitive files are excluded</small>
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
        <button
          className="workspace-switch"
          onClick={() => void chooseWorkspace()}
          disabled={busy !== null}
          title="Open another folder"
        >
          ⋯
        </button>
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
          <div className="workspace-preview-header">
            <div>
              <strong>{activeFile.path}</strong>
              <span>{activeFile.language} · {formatSize(activeFile.size)}</span>
            </div>
            <span className="context-badge">AI context</span>
          </div>
          <pre>
            <code>{activeFile.content}</code>
          </pre>
        </div>
      )}

      {error && <div className="workspace-error sticky">{error}</div>}
    </aside>
  );
}
