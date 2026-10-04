import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { WorkspaceSummary } from "./WorkspacePanel";
import "./memory.css";

type MemoryItem = {
  id: string;
  scope: "global" | "workspace";
  workspace: string | null;
  kind: "preference" | "instruction" | "project" | "workflow" | "context";
  content: string;
  pinned: boolean;
  enabled: boolean;
  source: string;
  createdAt: number;
  updatedAt: number;
  lastUsedAt: number | null;
  useCount: number;
};

type MemoryStats = {
  total: number;
  enabled: number;
  pinned: number;
  global: number;
  workspace: number;
};

type MemoryPanelProps = {
  workspace: WorkspaceSummary | null;
  onNotice: (message: string) => void;
};

const kinds: MemoryItem["kind"][] = [
  "preference",
  "instruction",
  "project",
  "workflow",
  "context",
];

const formatTime = (seconds: number) =>
  new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(new Date(seconds * 1000));

export default function MemoryPanel({ workspace, onNotice }: MemoryPanelProps) {
  const [items, setItems] = useState<MemoryItem[]>([]);
  const [stats, setStats] = useState<MemoryStats>({
    total: 0,
    enabled: 0,
    pinned: 0,
    global: 0,
    workspace: 0,
  });
  const [scope, setScope] = useState<"all" | "global" | "workspace">("all");
  const [query, setQuery] = useState("");
  const [content, setContent] = useState("");
  const [newScope, setNewScope] = useState<"global" | "workspace">(
    workspace ? "workspace" : "global",
  );
  const [kind, setKind] = useState<MemoryItem["kind"]>("instruction");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [memoryEnabled, setMemoryEnabled] = useState(
    () => localStorage.getItem("boscode.memory.enabled") !== "false",
  );
  const [autoCapture, setAutoCapture] = useState(
    () => localStorage.getItem("boscode.memory.autoCapture") !== "false",
  );

  const visible = useMemo(() => items, [items]);

  const load = async () => {
    try {
      const [memories, nextStats] = await Promise.all([
        invoke<MemoryItem[]>("list_memories", {
          scope,
          workspace: workspace?.root ?? null,
          query: query.trim() || null,
        }),
        invoke<MemoryStats>("memory_stats"),
      ]);
      setItems(memories);
      setStats(nextStats);
    } catch (caught) {
      setError(String(caught));
    }
  };

  useEffect(() => {
    void load();
  }, [scope, workspace?.root]);

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 180);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    if (!workspace && newScope === "workspace") setNewScope("global");
  }, [workspace?.root, newScope]);

  const create = async () => {
    const value = content.trim();
    if (!value) return;

    setBusy("create");
    setError("");

    try {
      await invoke("create_memory", {
        content: value,
        scope: newScope,
        workspace: newScope === "workspace" ? workspace?.root ?? null : null,
        kind,
        pinned: false,
      });
      setContent("");
      onNotice("Memory saved");
      await load();
    } catch (caught) {
      setError(String(caught));
      onNotice("Memory was not saved");
    } finally {
      setBusy(null);
    }
  };

  const update = async (
    item: MemoryItem,
    patch: {
      content?: string;
      kind?: MemoryItem["kind"];
      pinned?: boolean;
      enabled?: boolean;
    },
  ) => {
    setBusy(item.id);
    setError("");

    try {
      await invoke("update_memory", {
        id: item.id,
        content: patch.content ?? null,
        kind: patch.kind ?? null,
        pinned: patch.pinned ?? null,
        enabled: patch.enabled ?? null,
      });
      setEditing(null);
      setDraft("");
      onNotice("Memory updated");
      await load();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  const forget = async (item: MemoryItem) => {
    setBusy(item.id);
    setError("");

    try {
      await invoke("delete_memory", { id: item.id });
      onNotice("Memory forgotten");
      await load();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside className="right-panel memory-panel">
      <header className="memory-header">
        <div>
          <strong>Memory</strong>
          <span>Local persistent context for BOSCode</span>
        </div>
        <span className="memory-total">{stats.enabled}/{stats.total} active</span>
      </header>

      <div className="memory-master-controls">
        <label>
          <input
            type="checkbox"
            checked={memoryEnabled}
            onChange={(event) => {
              const enabled = event.target.checked;
              setMemoryEnabled(enabled);
              localStorage.setItem("boscode.memory.enabled", String(enabled));
              onNotice(enabled ? "BOSCode memory enabled" : "BOSCode memory disabled");
            }}
          />
          <span>
            <strong>Use memory</strong>
            <small>Recall relevant memories in future chats.</small>
          </span>
        </label>
        <label>
          <input
            type="checkbox"
            checked={autoCapture}
            onChange={(event) => {
              const enabled = event.target.checked;
              setAutoCapture(enabled);
              localStorage.setItem("boscode.memory.autoCapture", String(enabled));
              onNotice(enabled ? "Auto-remember enabled" : "Auto-remember disabled");
            }}
          />
          <span>
            <strong>Auto-remember</strong>
            <small>Capture strong memory-intent phrases automatically.</small>
          </span>
        </label>
      </div>

      <div className="memory-stats">
        <div><strong>{stats.global}</strong><span>Global</span></div>
        <div><strong>{stats.workspace}</strong><span>Workspace</span></div>
        <div><strong>{stats.pinned}</strong><span>Pinned</span></div>
      </div>

      <section className="memory-add">
        <textarea
          value={content}
          onChange={(event) => setContent(event.target.value)}
          placeholder="What should BOSCode remember?"
          spellCheck={false}
        />
        <div>
          <select
            value={newScope}
            onChange={(event) =>
              setNewScope(event.target.value as "global" | "workspace")
            }
          >
            <option value="global">Global</option>
            <option value="workspace" disabled={!workspace}>This workspace</option>
          </select>
          <select
            value={kind}
            onChange={(event) => setKind(event.target.value as MemoryItem["kind"])}
          >
            {kinds.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
          <button
            className="memory-primary"
            onClick={() => void create()}
            disabled={busy !== null || !content.trim()}
          >
            Remember
          </button>
        </div>
        <small>
          BOSCode blocks likely passwords, API keys, private keys, and access tokens from memory.
        </small>
      </section>

      <div className="memory-controls">
        <select
          value={scope}
          onChange={(event) =>
            setScope(event.target.value as "all" | "global" | "workspace")
          }
        >
          <option value="all">All memories</option>
          <option value="global">Global</option>
          <option value="workspace">This workspace</option>
        </select>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search memory…"
          spellCheck={false}
        />
      </div>

      {error && <div className="memory-error">{error}</div>}

      <div className="memory-list">
        {visible.length === 0 ? (
          <div className="memory-empty">
            <span>◎</span>
            <strong>No matching memories</strong>
            <p>
              Add one above, or tell BOSCode “remember that…”, “from now on…”, or
              “for this project…”.
            </p>
          </div>
        ) : (
          visible.map((item) => (
            <article
              key={item.id}
              className={`memory-card ${item.enabled ? "" : "disabled"}`}
            >
              <div className="memory-card-head">
                <div>
                  <span className={`memory-scope ${item.scope}`}>
                    {item.scope === "global" ? "Global" : "Workspace"}
                  </span>
                  <span className="memory-kind">{item.kind}</span>
                  {item.source === "conversation" && (
                    <span className="memory-source">auto</span>
                  )}
                </div>
                <button
                  className={item.pinned ? "memory-pin active" : "memory-pin"}
                  onClick={() => void update(item, { pinned: !item.pinned })}
                  disabled={busy !== null}
                  title={item.pinned ? "Unpin memory" : "Pin memory"}
                >
                  ◆
                </button>
              </div>

              {editing === item.id ? (
                <div className="memory-edit">
                  <textarea
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    spellCheck={false}
                  />
                  <div>
                    <button onClick={() => setEditing(null)}>Cancel</button>
                    <button
                      className="memory-primary"
                      onClick={() => void update(item, { content: draft })}
                      disabled={!draft.trim() || busy !== null}
                    >
                      Save
                    </button>
                  </div>
                </div>
              ) : (
                <p>{item.content}</p>
              )}

              <footer>
                <span>
                  {formatTime(item.updatedAt)}
                  {item.useCount > 0 ? ` · recalled ${item.useCount}×` : ""}
                </span>
                <div>
                  <button
                    onClick={() => void update(item, { enabled: !item.enabled })}
                    disabled={busy !== null}
                  >
                    {item.enabled ? "Disable" : "Enable"}
                  </button>
                  <button
                    onClick={() => {
                      setEditing(item.id);
                      setDraft(item.content);
                    }}
                    disabled={busy !== null}
                  >
                    Edit
                  </button>
                  <button
                    className="memory-forget"
                    onClick={() => void forget(item)}
                    disabled={busy !== null}
                  >
                    Forget
                  </button>
                </div>
              </footer>
            </article>
          ))
        )}
      </div>
    </aside>
  );
}
