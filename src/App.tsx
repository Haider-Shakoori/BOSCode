import { useMemo, useState } from "react";
import ProviderSettings from "./ProviderSettings";

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

const steps = [
  ["done", "Reading Sale.php"],
  ["done", "Reading Invoice.php"],
  ["done", "Found rounding issue in calculation"],
  ["done", "Applying fix to Sale.php"],
  ["done", "Adding unit tests"],
  ["running", "Running test suite..."],
  ["waiting", "Review results and adjust if needed"],
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
  const [message, setMessage] = useState("");
  const [provider, setProvider] = useState("Big Pickle");
  const [notice, setNotice] = useState("Foundation ready");
  const [settingsOpen, setSettingsOpen] = useState(false);

  const stats = useMemo(
    () => [["3", "files changed"], ["143", "tests passing"], ["12.34s", "last run"]],
    [],
  );

  const sendMessage = () => {
    const prompt = message.trim();
    if (!prompt) return;
    setNotice(`Queued: ${prompt.slice(0, 42)}${prompt.length > 42 ? "…" : ""}`);
    setMessage("");
  };

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

          <div className="chat-panel">
            <article className="message user-message">
              <div className="message-avatar user">H</div>
              <div>
                <strong>You</strong>
                <p>Fix the checkout rounding issue and add tests. Then run the test suite and make sure everything passes.</p>
              </div>
            </article>
            <article className="message agent-message">
              <div className="message-avatar agent"><BrandMark /></div>
              <div className="message-body">
                <div className="agent-title"><strong>BOSCode</strong><span>{provider}</span><i /></div>
                <p>I’ll inspect the checkout calculation, identify the rounding issue, implement the fix, and validate it with tests.</p>
                <div className="task-steps">
                  {steps.map(([state, label]) => (
                    <div className="task-step" key={label}>
                      <span className={`step-icon ${state}`}>{state === "done" ? "✓" : ""}</span>
                      <span>{label}</span>
                    </div>
                  ))}
                </div>
              </div>
            </article>
          </div>

          <div className="composer">
            <textarea
              value={message}
              onChange={(event) => setMessage(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  sendMessage();
                }
              }}
              placeholder="Ask BOSCode anything…"
              rows={1}
            />
            <div className="composer-actions">
              <select value={provider} onChange={(event) => setProvider(event.target.value)}>
                <option>Big Pickle</option>
                <option>OpenAI</option>
                <option>Claude</option>
                <option>Gemini</option>
                <option>Ollama</option>
              </select>
              <button className="attach" title="Attach context">⌁</button>
              <button className="send" onClick={sendMessage} aria-label="Send">➜</button>
            </div>
          </div>

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
