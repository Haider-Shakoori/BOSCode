import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { WorkspaceSummary } from "./WorkspacePanel";
import "./sessions.css";

export type SessionSummary = {
  id: string;
  title: string;
  workspace: string | null;
  provider: string | null;
  model: string | null;
  pinned: boolean;
  archived: boolean;
  summary: string | null;
  createdAt: number;
  updatedAt: number;
  lastOpenedAt: number;
  messageCount: number;
  preview: string | null;
};

type SessionSidebarProps = {
  workspace: WorkspaceSummary | null;
  activeSessionId: string | null;
  onSelect: (session: SessionSummary) => void;
  onCreated: (session: SessionSummary) => void;
  onNotice: (message: string) => void;
};

const ageLabel = (seconds: number) => {
  const delta = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (delta < 60) return "now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h`;
  if (delta < 604800) return `${Math.floor(delta / 86400)}d`;
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(
    new Date(seconds * 1000),
  );
};

export default function SessionSidebar({
  workspace,
  activeSessionId,
  onSelect,
  onCreated,
  onNotice,
}: SessionSidebarProps) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [query, setQuery] = useState("");
  const [showArchived, setShowArchived] = useState(false);
  const [menu, setMenu] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [titleDraft, setTitleDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const load = async () => {
    try {
      const result = await invoke<SessionSummary[]>("list_sessions", {
        workspace: workspace?.root ?? null,
        query: query.trim() || null,
        includeArchived: showArchived,
      });
      setSessions(result);
      setError("");
    } catch (caught) {
      setError(String(caught));
    }
  };

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 150);
    return () => window.clearTimeout(timer);
  }, [workspace?.root, query, showArchived]);

  const create = async () => {
    if (busy) return;
    setBusy(true);
    setError("");

    try {
      const session = await invoke<SessionSummary>("create_session", {
        title: null,
        workspace: workspace?.root ?? null,
        provider: null,
        model: null,
      });
      setSessions((current) => [session, ...current]);
      onCreated(session);
      onNotice("New coding session");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const update = async (
    session: SessionSummary,
    action: "pin" | "archive" | "rename" | "delete",
  ) => {
    if (busy) return;
    setBusy(true);
    setError("");

    try {
      if (action === "delete") {
        await invoke<boolean>("delete_session", { id: session.id });
        setSessions((current) => current.filter((item) => item.id !== session.id));
        onNotice("Coding session deleted");
      } else if (action === "rename") {
        const updated = await invoke<SessionSummary>("rename_session", {
          id: session.id,
          title: titleDraft,
        });
        setSessions((current) =>
          current.map((item) => (item.id === updated.id ? updated : item)),
        );
        setEditing(null);
        setTitleDraft("");
        onNotice("Session renamed");
      } else if (action === "pin") {
        const updated = await invoke<SessionSummary>("set_session_pinned", {
          id: session.id,
          pinned: !session.pinned,
        });
        await load();
        onNotice(updated.pinned ? "Session pinned" : "Session unpinned");
      } else {
        await invoke<SessionSummary>("set_session_archived", {
          id: session.id,
          archived: !session.archived,
        });
        await load();
        onNotice(session.archived ? "Session restored" : "Session archived");
      }
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
      setMenu(null);
    }
  };

  const grouped = useMemo(() => {
    const pinned = sessions.filter((session) => session.pinned && !session.archived);
    const active = sessions.filter((session) => !session.pinned && !session.archived);
    const archived = sessions.filter((session) => session.archived);
    return { pinned, active, archived };
  }, [sessions]);

  const renderSession = (session: SessionSummary) => (
    <div
      className={session.id === activeSessionId ? "history-session active" : "history-session"}
      key={session.id}
    >
      {editing === session.id ? (
        <div className="session-rename-row">
          <input
            value={titleDraft}
            onChange={(event) => setTitleDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && titleDraft.trim()) {
                void update(session, "rename");
              }
              if (event.key === "Escape") setEditing(null);
            }}
            autoFocus
          />
          <button
            onClick={() => void update(session, "rename")}
            disabled={!titleDraft.trim() || busy}
          >
            ✓
          </button>
        </div>
      ) : (
        <button className="history-session-main" onClick={() => onSelect(session)}>
          <span>{session.pinned ? "◆" : "▧"}</span>
          <span>
            <strong>{session.title}</strong>
            <small>
              {session.messageCount} messages
              {session.model ? ` · ${session.model}` : ""}
            </small>
          </span>
          <time>{ageLabel(session.updatedAt)}</time>
        </button>
      )}

      <button
        className="history-session-menu"
        onClick={() => setMenu((current) => (current === session.id ? null : session.id))}
        title="Session actions"
      >
        ⋯
      </button>

      {menu === session.id && (
        <div className="history-session-popover">
          <button onClick={() => void update(session, "pin")}>
            {session.pinned ? "Unpin" : "Pin"}
          </button>
          <button
            onClick={() => {
              setEditing(session.id);
              setTitleDraft(session.title);
              setMenu(null);
            }}
          >
            Rename
          </button>
          <button onClick={() => void update(session, "archive")}>
            {session.archived ? "Restore" : "Archive"}
          </button>
          <button className="danger" onClick={() => void update(session, "delete")}>
            Delete
          </button>
        </div>
      )}
    </div>
  );

  return (
    <section className="history-sidebar">
      <div className="history-heading">
        <span>SESSIONS</span>
        <button onClick={() => void create()} disabled={busy}>＋ New</button>
      </div>

      <div className="history-search">
        <span>⌕</span>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search sessions…"
          spellCheck={false}
        />
      </div>

      {error && <div className="history-error">{error}</div>}

      <div className="history-list">
        {grouped.pinned.length > 0 && (
          <div className="history-group">
            <small>PINNED</small>
            {grouped.pinned.map(renderSession)}
          </div>
        )}

        <div className="history-group">
          {grouped.pinned.length > 0 && <small>RECENT</small>}
          {grouped.active.length > 0 ? (
            grouped.active.map(renderSession)
          ) : (
            <p className="history-empty">No matching coding sessions.</p>
          )}
        </div>

        {showArchived && grouped.archived.length > 0 && (
          <div className="history-group">
            <small>ARCHIVED</small>
            {grouped.archived.map(renderSession)}
          </div>
        )}
      </div>

      <label className="history-archive-toggle">
        <input
          type="checkbox"
          checked={showArchived}
          onChange={(event) => setShowArchived(event.target.checked)}
        />
        <span>Show archived sessions</span>
      </label>
    </section>
  );
}
