import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { WorkspaceSummary } from "./WorkspacePanel";

export type PendingChange = {
  id: string;
  path: string;
  action: "create" | "update" | "delete";
  originalContent: string | null;
  proposedContent: string | null;
  diff: string;
  additions: number;
  deletions: number;
};

type PermissionStatus = {
  mode: "read-only" | "workspace-write";
  canWrite: boolean;
};

type ChangesPanelProps = {
  workspace: WorkspaceSummary | null;
  refreshToken: number;
  onNotice: (message: string) => void;
  onWorkspaceRefresh: () => Promise<void>;
};

const actionLabel: Record<PendingChange["action"], string> = {
  create: "A",
  update: "M",
  delete: "D",
};

export default function ChangesPanel({
  workspace,
  refreshToken,
  onNotice,
  onWorkspaceRefresh,
}: ChangesPanelProps) {
  const [changes, setChanges] = useState<PendingChange[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [permission, setPermission] = useState<PermissionStatus>({
    mode: "read-only",
    canWrite: false,
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");

  const selected = useMemo(
    () => changes.find((change) => change.id === selectedId) ?? changes[0] ?? null,
    [changes, selectedId],
  );

  const load = async () => {
    if (!workspace) {
      setChanges([]);
      setSelectedId(null);
      return;
    }

    try {
      const [pending, status] = await Promise.all([
        invoke<PendingChange[]>("list_pending_changes"),
        invoke<PermissionStatus>("get_permission_mode"),
      ]);
      setChanges(pending);
      setPermission(status);
      setSelectedId((current) =>
        current && pending.some((change) => change.id === current)
          ? current
          : pending[0]?.id ?? null,
      );
    } catch (caught) {
      setError(String(caught));
    }
  };

  useEffect(() => {
    void load();
  }, [workspace?.root, refreshToken]);

  const updatePermission = async (mode: PermissionStatus["mode"]) => {
    setBusy("permission");
    setError("");

    try {
      const status = await invoke<PermissionStatus>("set_permission_mode", { mode });
      setPermission(status);
      onNotice(
        status.canWrite
          ? "Workspace Write enabled — Apply and Undo can modify project files"
          : "Read-only mode enabled",
      );
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  const apply = async (change: PendingChange) => {
    setBusy(change.id);
    setError("");

    try {
      await invoke("apply_workspace_change", { changeId: change.id });
      onNotice(`Applied ${change.path}`);
      await load();
      await onWorkspaceRefresh();
    } catch (caught) {
      setError(String(caught));
      onNotice("Change was not applied");
    } finally {
      setBusy(null);
    }
  };

  const reject = async (change: PendingChange) => {
    setBusy(change.id);
    setError("");

    try {
      await invoke("reject_workspace_change", { changeId: change.id });
      onNotice(`Rejected ${change.path}`);
      await load();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  const applyAll = async () => {
    if (!permission.canWrite || changes.length === 0) return;
    setBusy("all");
    setError("");

    try {
      for (const change of changes) {
        await invoke("apply_workspace_change", { changeId: change.id });
      }
      onNotice(`Applied ${changes.length} pending change${changes.length === 1 ? "" : "s"}`);
      await load();
      await onWorkspaceRefresh();
    } catch (caught) {
      setError(String(caught));
      onNotice("Apply All stopped on a protected or stale change");
      await load();
      await onWorkspaceRefresh();
    } finally {
      setBusy(null);
    }
  };

  const undo = async () => {
    setBusy("undo");
    setError("");

    try {
      const result = await invoke<{ path: string }>("undo_last_workspace_change");
      onNotice(`Undid BOSCode change to ${result.path}`);
      await onWorkspaceRefresh();
    } catch (caught) {
      setError(String(caught));
      onNotice("Nothing was undone");
    } finally {
      setBusy(null);
    }
  };

  if (!workspace) {
    return (
      <aside className="right-panel changes-panel changes-empty">
        <div className="workspace-empty-card">
          <div className="workspace-empty-icon">⑂</div>
          <h3>No workspace</h3>
          <p>Open a project before reviewing or applying BOSCode changes.</p>
        </div>
      </aside>
    );
  }

  return (
    <aside className="right-panel changes-panel">
      <div className="changes-toolbar">
        <div>
          <strong>Changes</strong>
          <span>{changes.length} pending</span>
        </div>
        <div className="permission-control">
          <span className={permission.canWrite ? "permission-dot write" : "permission-dot"} />
          <select
            value={permission.mode}
            onChange={(event) =>
              void updatePermission(event.target.value as PermissionStatus["mode"])
            }
            disabled={busy !== null}
            aria-label="Workspace permission mode"
          >
            <option value="read-only">Read-only</option>
            <option value="workspace-write">Workspace Write</option>
          </select>
        </div>
      </div>

      <div className="changes-actions">
        <button
          className="primary"
          onClick={() => void applyAll()}
          disabled={!permission.canWrite || changes.length === 0 || busy !== null}
        >
          {busy === "all" ? "Applying…" : "Apply All"}
        </button>
        <button
          onClick={() => void undo()}
          disabled={!permission.canWrite || busy !== null}
          title="Undo the most recent BOSCode-applied file change"
        >
          Undo Last
        </button>
      </div>

      {error && <div className="workspace-error changes-error">{error}</div>}

      {changes.length === 0 ? (
        <div className="changes-empty-state">
          <div>✓</div>
          <strong>No pending changes</strong>
          <p>Edit a file from Explorer and choose “Propose Change”. BOSCode will stage the patch here before anything touches disk.</p>
        </div>
      ) : (
        <>
          <div className="pending-change-list">
            {changes.map((change) => (
              <button
                key={change.id}
                className={selected?.id === change.id ? "active" : ""}
                onClick={() => setSelectedId(change.id)}
              >
                <span className={`pending-action ${change.action}`}>
                  {actionLabel[change.action]}
                </span>
                <span className="pending-path">{change.path}</span>
                <span className="pending-stat">
                  <i>+{change.additions}</i> <b>-{change.deletions}</b>
                </span>
              </button>
            ))}
          </div>

          {selected && (
            <section className="pending-diff-card">
              <header>
                <div>
                  <strong>{selected.path}</strong>
                  <span>{selected.action}</span>
                </div>
                <div>
                  <button
                    onClick={() => void reject(selected)}
                    disabled={busy !== null}
                  >
                    Reject
                  </button>
                  <button
                    className="primary"
                    onClick={() => void apply(selected)}
                    disabled={!permission.canWrite || busy !== null}
                  >
                    {busy === selected.id ? "Applying…" : "Apply"}
                  </button>
                </div>
              </header>
              {!permission.canWrite && (
                <div className="write-lock-note">
                  Review is available now. Switch to <strong>Workspace Write</strong> to apply.
                </div>
              )}
              <pre className="unified-diff">
                <code>{selected.diff || "No textual diff available."}</code>
              </pre>
            </section>
          )}
        </>
      )}
    </aside>
  );
}
