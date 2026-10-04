import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ProviderSettings from "./ProviderSettings";
import ChatWorkspace from "./ChatWorkspace";
import ChangesPanel from "./ChangesPanel";
import TerminalPanel from "./TerminalPanel";
import GitPanel from "./GitPanel";
import WorkspacePanel, { type WorkspaceEntry, type WorkspaceFile, type WorkspaceSummary } from "./WorkspacePanel";

type NavItem = { icon: string; label: string; badge?: string };

const navItems: NavItem[] = [
  { icon: "✦", label: "Chat" },
  { icon: "▱", label: "Explorer" },
  { icon: "⌕", label: "Search" },
  { icon: "⑂", label: "Source Control", badge: "3" },
  { icon: "›_", label: "Terminal" },
  { icon: "✓", label: "Tasks", badge: "2" },
];

const sessions = [
  ["Fix checkout rounding", "2m"],
  ["Implement loyalty system", "1h"],
  ["Optimize queries", "3h"],
  ["Add inventory report", "1d"],
];



function BrandMark() {
  return (
    <div className="brand-mark" aria-hidden="true">
      <span />
      <span />
    </div>
  );
}

export default function App() {
  const [activeNav, setActiveNav] = useState("Chat");
  const [activeTab, setActiveTab] = useState("Chat");
  const [provider, setProvider] = useState("Big Pickle");
  const [notice, setNotice] = useState("Foundation ready");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [workspace, setWorkspace] = useState<WorkspaceSummary | null>(null);
  const [workspaceEntries, setWorkspaceEntries] = useState<WorkspaceEntry[]>([]);
  const [activeFile, setActiveFile] = useState<WorkspaceFile | null>(null);
  const [changesRefreshToken, setChangesRefreshToken] = useState(0);


  useEffect(() => {
    const savedPath = localStorage.getItem("boscode.workspace.path");
    if (!savedPath) return;

    let cancelled = false;

    const restoreWorkspace = async () => {
      try {
        const summary = await invoke<WorkspaceSummary>("set_workspace", { path: savedPath });
        const entries = await invoke<WorkspaceEntry[]>("list_workspace");
        if (!cancelled) {
          setWorkspace(summary);
          setWorkspaceEntries(entries);
          setNotice(`Workspace restored: ${summary.name}`);
        }
      } catch {
        localStorage.removeItem("boscode.workspace.path");
      }
    };

    void restoreWorkspace();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleWorkspaceChange = (summary: WorkspaceSummary, entries: WorkspaceEntry[]) => {
    setWorkspace(summary);
    setWorkspaceEntries(entries);
    setActiveFile(null);
    setActiveNav("Explorer");
  };


  const refreshWorkspace = async () => {
    if (!workspace) return;

    const [summary, entries] = await Promise.all([
      invoke<WorkspaceSummary>("get_workspace"),
      invoke<WorkspaceEntry[]>("list_workspace"),
    ]);

    setWorkspace(summary);
    setWorkspaceEntries(entries);

    if (activeFile) {
      try {
        const refreshed = await invoke<WorkspaceFile>("read_workspace_file", {
          path: activeFile.path,
        });
        setActiveFile(refreshed);
      } catch {
        setActiveFile(null);
      }
    }
  };

  const handleChangeProposed = () => {
    setChangesRefreshToken((value) => value + 1);
    setActiveNav("Source Control");
  };



  return (
    <main className="app-shell">
      <header className="titlebar">
        <div className="brand">
          <BrandMark />
          <strong>BOSCode</strong>
          <span>AI Coding Agent by BusinessOS</span>
        </div>
        <button
          className="workspace-pill"
          onClick={() => setActiveNav("Explorer")}
          title={workspace?.root ?? "Open a workspace from the Explorer panel"}
        >
          <span className="status-dot" />
          <span>{workspace?.name ?? "No workspace"}</span>
          <small>{workspace?.branch ?? "local"}</small>
          <span>⌄</span>
        </button>
        <div className="window-actions" aria-label="Workspace actions">
          <button title="Command palette">⌘</button>
          <button title="Notifications">◌</button>
          <button title="Settings" onClick={() => setSettingsOpen(true)}>⚙</button>
        </div>
      </header>

      <section className="workspace">
        <aside className="sidebar">
          <nav className="primary-nav">
            {navItems.map((item) => (
              <button
                key={item.label}
                className={activeNav === item.label ? "nav-item active" : "nav-item"}
                onClick={() => {
                  setActiveNav(item.label);
                  setNotice(`${item.label} selected`);
                }}
              >
                <span className="nav-icon">{item.icon}</span>
                <span>{item.label}</span>
                {item.badge && <small className="badge">{item.badge}</small>}
              </button>
            ))}
          </nav>

          <div className="sidebar-section">
            <div className="section-heading">
              <span>SESSIONS</span>
              <button onClick={() => setNotice("New coding session")}>＋ New</button>
            </div>
            <div className="session-list">
              {sessions.map(([label, age], index) => (
                <button
                  key={label}
                  className={index === 0 ? "session active" : "session"}
                  onClick={() => setNotice(`Opened session: ${label}`)}
                >
                  <span>▧</span>
                  <span className="session-name">{label}</span>
                  <small>{age}</small>
                </button>
              ))}
            </div>
          </div>

          <div className="sidebar-footer">
            <button className="nav-item" onClick={() => setSettingsOpen(true)}>
              <span className="nav-icon">⚙</span><span>Settings</span>
            </button>
            <div className="profile-card">
              <div className="avatar">B</div>
              <div><strong>BusinessOS</strong><span>Local workspace</span></div>
            </div>
          </div>
        </aside>

        <section className="center-column">
          <div className="top-tabs">
            {["Chat", "Files", "Terminal", "Git", "Tasks"].map((tab) => (
              <button
                key={tab}
                className={activeTab === tab ? "tab active" : "tab"}
                onClick={() => setActiveTab(tab)}
              >
                {tab}
                {tab === "Files" && <small>3</small>}
                {tab === "Tasks" && <small>2</small>}
              </button>
            ))}
          </div>

          {activeTab === "Git" ? (
            <GitPanel
              workspace={workspace}
              onNotice={setNotice}
              onWorkspaceRefresh={refreshWorkspace}
            />
          ) : (
            <>
              <ChatWorkspace
                provider={provider}
                onProviderChange={setProvider}
                onNotice={setNotice}
                onOpenSettings={() => setSettingsOpen(true)}
                workspace={workspace}
                activeFile={activeFile}
              />

              <TerminalPanel
                workspace={workspace}
                onNotice={setNotice}
              />
            </>
          )}
        </section>

        {activeNav === "Source Control" ? (
          <ChangesPanel
            workspace={workspace}
            refreshToken={changesRefreshToken}
            onNotice={setNotice}
            onWorkspaceRefresh={refreshWorkspace}
          />
        ) : (
          <WorkspacePanel
            workspace={workspace}
            entries={workspaceEntries}
            activeFile={activeFile}
            onWorkspaceChange={handleWorkspaceChange}
            onFileOpen={setActiveFile}
            onNotice={setNotice}
            onChangeProposed={handleChangeProposed}
            mode={activeNav === "Search" ? "search" : activeNav === "Explorer" ? "files" : null}
          />
        )}
      </section>

      {settingsOpen && (
        <ProviderSettings
          activeProvider={provider}
          onProviderChange={setProvider}
          onClose={() => setSettingsOpen(false)}
          onNotice={setNotice}
        />
      )}

      <footer className="statusbar">
        <span><i className="status-dot" /> {notice}</span>
        <span>⑂ {workspace?.branch ?? "no workspace"}</span><span>UTF-8</span><span>Spaces: 2</span><span>BOSCode 0.1.0</span>
      </footer>
    </main>
  );
}
