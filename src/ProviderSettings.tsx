import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";

type ProviderSettingsProps = {
  activeProvider: string;
  onProviderChange: (provider: string) => void;
  onClose: () => void;
  onNotice: (message: string) => void;
};

type ProviderDefinition = {
  id: string;
  name: string;
  badge: string;
  baseUrl: string;
  model: string;
  keyRequired: boolean;
  note: string;
};

type StoredProviderConfig = {
  baseUrl: string;
  model: string;
};

type ProviderTestResult = {
  provider: string;
  model: string;
  status: number;
  latency_ms: number;
  message: string;
};

const providers: ProviderDefinition[] = [
  {
    id: "big-pickle",
    name: "Big Pickle",
    badge: "BP",
    baseUrl: "https://opencode.ai/zen/v1",
    model: "big-pickle",
    keyRequired: true,
    note: "Default BOSCode provider through OpenCode Zen. Free-period prompts may be used by the provider to improve the model.",
  },
  {
    id: "openai",
    name: "OpenAI",
    badge: "OA",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-5.6",
    keyRequired: true,
    note: "Direct OpenAI-compatible provider.",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    badge: "OR",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "",
    keyRequired: true,
    note: "Use any OpenAI-compatible model exposed by OpenRouter.",
  },
  {
    id: "ollama",
    name: "Ollama",
    badge: "OL",
    baseUrl: "http://localhost:11434/v1",
    model: "",
    keyRequired: false,
    note: "Local models. API key is optional.",
  },
  {
    id: "custom",
    name: "Custom",
    badge: "↗",
    baseUrl: "",
    model: "",
    keyRequired: false,
    note: "Any OpenAI-compatible chat completions endpoint.",
  },
];

const storageKey = (providerId: string) => `boscode.provider.${providerId}`;

export default function ProviderSettings({
  activeProvider,
  onProviderChange,
  onClose,
  onNotice,
}: ProviderSettingsProps) {
  const activeDefinition =
    providers.find((item) => item.name === activeProvider) ?? providers[0];

  const [selectedId, setSelectedId] = useState(activeDefinition.id);
  const selected = useMemo(
    () => providers.find((item) => item.id === selectedId) ?? providers[0],
    [selectedId],
  );

  const [baseUrl, setBaseUrl] = useState(selected.baseUrl);
  const [model, setModel] = useState(selected.model);
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [hasStoredKey, setHasStoredKey] = useState(false);
  const [busy, setBusy] = useState<"save" | "test" | "delete" | null>(null);
  const [result, setResult] = useState<ProviderTestResult | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const fallback = { baseUrl: selected.baseUrl, model: selected.model };
    const raw = localStorage.getItem(storageKey(selected.id));
    let stored: StoredProviderConfig = fallback;

    if (raw) {
      try {
        stored = { ...fallback, ...JSON.parse(raw) };
      } catch {
        stored = fallback;
      }
    }

    setBaseUrl(stored.baseUrl);
    setModel(stored.model);
    setApiKey("");
    setResult(null);
    setError("");

    invoke<boolean>("provider_secret_exists", { providerId: selected.id })
      .then(setHasStoredKey)
      .catch(() => setHasStoredKey(false));
  }, [selected]);

  const saveSettings = async () => {
    setBusy("save");
    setError("");

    try {
      localStorage.setItem(storageKey(selected.id), JSON.stringify({ baseUrl, model }));

      if (apiKey.trim()) {
        await invoke("save_provider_secret", {
          providerId: selected.id,
          secret: apiKey.trim(),
        });
        setHasStoredKey(true);
        setApiKey("");
      }

      onProviderChange(selected.name);
      onNotice(`${selected.name} provider settings saved`);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  const testConnection = async () => {
    setBusy("test");
    setError("");
    setResult(null);

    try {
      const tested = await invoke<ProviderTestResult>("test_provider_connection", {
        providerId: selected.id,
        baseUrl: baseUrl.trim(),
        model: model.trim(),
        apiKey: apiKey.trim() || null,
      });
      setResult(tested);
      onNotice(`${selected.name} connection verified`);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  const deleteKey = async () => {
    setBusy("delete");
    setError("");

    try {
      await invoke("delete_provider_secret", { providerId: selected.id });
      setHasStoredKey(false);
      setApiKey("");
      onNotice(`${selected.name} API key removed`);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="settings-window"
        role="dialog"
        aria-modal="true"
        aria-label="BOSCode settings"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="settings-titlebar">
          <div>
            <strong>Settings</strong>
            <span>Configure BOSCode for your local development environment.</span>
          </div>
          <button onClick={onClose} aria-label="Close settings">×</button>
        </header>

        <div className="settings-layout">
          <aside className="settings-nav">
            <button>General</button>
            <button className="active">AI Providers</button>
            <button>Models</button>
            <button>Appearance</button>
            <button>Terminal</button>
            <button>Git & GitHub</button>
            <button>Shortcuts</button>
            <button>Updates</button>
          </aside>

          <div className="settings-content">
            <div className="settings-heading">
              <div>
                <h2>AI Providers</h2>
                <p>Choose the model service BOSCode uses for coding sessions.</p>
              </div>
              <span className="secure-pill">Secure local storage</span>
            </div>

            <div className="provider-settings-grid">
              <div className="provider-list">
                {providers.map((item) => (
                  <button
                    key={item.id}
                    className={selected.id === item.id ? "provider-row active" : "provider-row"}
                    onClick={() => setSelectedId(item.id)}
                  >
                    <span className="provider-logo">{item.badge}</span>
                    <span className="provider-copy">
                      <strong>{item.name}</strong>
                      <small>
                        {item.id === activeDefinition.id ? "Current provider" : "Available"}
                      </small>
                    </span>
                    {item.id === activeDefinition.id && <i className="provider-check">✓</i>}
                  </button>
                ))}
              </div>

              <div className="provider-form">
                <div className="provider-form-header">
                  <span className="provider-logo large">{selected.badge}</span>
                  <div>
                    <h3>{selected.name}</h3>
                    <p>{selected.note}</p>
                  </div>
                </div>

                <label>
                  <span>API Base URL</span>
                  <input
                    value={baseUrl}
                    onChange={(event) => setBaseUrl(event.target.value)}
                    placeholder="https://provider.example/v1"
                    spellCheck={false}
                  />
                </label>

                <label>
                  <span>Model</span>
                  <input
                    value={model}
                    onChange={(event) => setModel(event.target.value)}
                    placeholder="Model ID"
                    spellCheck={false}
                  />
                </label>

                <label>
                  <span>
                    API Key
                    {hasStoredKey && <small className="stored-key">Stored securely</small>}
                  </span>
                  <div className="secret-input">
                    <input
                      type={showKey ? "text" : "password"}
                      value={apiKey}
                      onChange={(event) => setApiKey(event.target.value)}
                      placeholder={
                        hasStoredKey
                          ? "A key is already stored — enter a new one to replace it"
                          : selected.keyRequired
                            ? "Paste API key"
                            : "Optional"
                      }
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button onClick={() => setShowKey((value) => !value)}>
                      {showKey ? "Hide" : "Show"}
                    </button>
                  </div>
                </label>

                <div className="provider-security-note">
                  <span>◈</span>
                  <p>
                    API keys are stored by the operating system credential manager, not in
                    BOSCode project files or localStorage.
                  </p>
                </div>

                {result && (
                  <div className="connection-result success">
                    <strong>Connection verified</strong>
                    <span>
                      HTTP {result.status} · {result.model} · {result.latency_ms} ms
                    </span>
                  </div>
                )}

                {error && (
                  <div className="connection-result error">
                    <strong>Connection failed</strong>
                    <span>{error}</span>
                  </div>
                )}

                <div className="provider-actions">
                  {hasStoredKey && (
                    <button
                      className="danger-button"
                      onClick={deleteKey}
                      disabled={busy !== null}
                    >
                      {busy === "delete" ? "Removing…" : "Remove key"}
                    </button>
                  )}
                  <span />
                  <button
                    className="secondary-button"
                    onClick={testConnection}
                    disabled={busy !== null || !baseUrl.trim() || !model.trim()}
                  >
                    {busy === "test" ? "Testing…" : "Test Connection"}
                  </button>
                  <button
                    className="primary-button"
                    onClick={saveSettings}
                    disabled={busy !== null || !baseUrl.trim() || !model.trim()}
                  >
                    {busy === "save" ? "Saving…" : "Save Provider"}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
