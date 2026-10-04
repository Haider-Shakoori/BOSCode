import { useMemo, useState } from "react";
import ProviderSettings from "./ProviderSettings";
import ChatWorkspace from "./ChatWorkspace";

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


const files = [
  { path: "app/Models/Sale.php", type: "M", tone: "orange" },
  { path: "app/Models/Invoice.php", type: "M", tone: "orange" },
  { path: "tests/Feature/CheckoutTest.php", type: "A", tone: "green" },
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

  const stats = useMemo(
    () => [["3", "files changed"], ["143", "tests passing"], ["12.34s", "last run"]],
    [],
  );


  return (
    <main className="app-shell">
      <header className="titlebar">
        <div className="brand">
          <BrandMark />
          <strong>BOSCode</strong>
          <span>AI Coding Agent by BusinessOS</span>
        </div>
        <div className="workspace-pill">
          <span className="status-dot" />
          <span>POS System</span>
          <small>main</small>
          <span>⌄</span>
        </div>
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

          <ChatWorkspace
            provider={provider}
            onProviderChange={setProvider}
            onNotice={setNotice}
            onOpenSettings={() => setSettingsOpen(true)}
          />

          <section className="terminal-panel">
            <div className="panel-toolbar">
              <div className="terminal-tabs"><button className="active">Terminal ×</button><button>＋</button></div>
              <span>PowerShell⌄</span>
            </div>
            <div className="terminal-content">
              <p><span className="muted">PS C:\projects\businessos&gt;</span> <strong>php artisan test</strong></p>
              <p><span className="pass-label">PASS</span> <span className="muted">Tests\Feature\CheckoutTest</span></p>
              <p className="terminal-success">✓ it calculates tax correctly</p>
              <p className="terminal-success">✓ it handles rounding properly</p>
              <p className="terminal-success">✓ it applies discounts correctly</p>
              <p className="terminal-summary">Tests: <strong>143 passed</strong> <span>(612 assertions)</span></p>
            </div>
          </section>
        </section>

        <aside className="right-panel">
          <div className="changes-header">
            <div><strong>Changes</strong><span>(3 files)</span></div>
            <div>
              <button className="primary" onClick={() => setNotice("All changes approved")}>Apply All</button>
              <button onClick={() => setNotice("Changes rejected")}>Reject</button>
            </div>
          </div>

          <div className="file-change-list">
            {files.map((file) => (
              <button key={file.path}>
                <span className={`file-state ${file.tone}`}>{file.type}</span>
                <span>{file.path}</span>
              </button>
            ))}
          </div>

          <section className="diff-card">
            <div className="diff-title">
              <span><b>M</b> app/Models/Sale.php</span>
              <span className="diff-stat">+24 <i>-12</i></span>
            </div>
            <div className="diff-grid">
              <div className="code before">
                <span>145</span><code>public function calculateTotal()</code>
                <span>146</span><code>{"{"}</code>
                <span>147</span><code>$subtotal = $this-&gt;subtotal;</code>
                <span>148</span><code className="removed">$tax = $subtotal * $this-&gt;tax;</code>
                <span>149</span><code className="removed">$total = $subtotal + $tax;</code>
                <span>150</span><code>return $total;</code>
              </div>
              <div className="code after">
                <span>145</span><code>public function calculateTotal()</code>
                <span>146</span><code>{"{"}</code>
                <span>147</span><code>$subtotal = $this-&gt;subtotal;</code>
                <span>148</span><code className="added">$tax = round($subtotal * $this-&gt;tax, 3);</code>
                <span>149</span><code className="added">$total = round($subtotal + $tax, 3);</code>
                <span>150</span><code>return $total;</code>
              </div>
            </div>
            <div className="diff-footer"><button className="active">Side by Side</button><button>Unified</button><button>View File</button></div>
          </section>

          <section className="insight-card">
            <div className="insight-heading"><span>Run health</span><strong>Ready</strong></div>
            <div className="stat-row">
              {stats.map(([value, label]) => (
                <div key={label}><strong>{value}</strong><span>{label}</span></div>
              ))}
            </div>
          </section>
        </aside>
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
        <span>⑂ main</span><span>UTF-8</span><span>Spaces: 2</span><span>BOSCode 0.1.0</span>
      </footer>
    </main>
  );
}
