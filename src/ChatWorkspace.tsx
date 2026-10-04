import { Channel, invoke } from "@tauri-apps/api/core";
import { Fragment, useEffect, useState } from "react";
import type { WorkspaceFile, WorkspaceSummary } from "./WorkspacePanel";

type ChatWorkspaceProps = {
  provider: string;
  onProviderChange: (provider: string) => void;
  onNotice: (message: string) => void;
  onOpenSettings: () => void;
  workspace: WorkspaceSummary | null;
  activeFile: WorkspaceFile | null;
};

type Message = {
  id: string;
  role: "user" | "assistant";
  content: string;
  streaming?: boolean;
  error?: boolean;
};

type StreamEvent =
  | { type: "started"; model: string }
  | { type: "delta"; content: string }
  | { type: "completed" }
  | { type: "error"; message: string };

type MemoryCaptureResult = {
  captured: number;
  skippedSensitive: boolean;
};

const providerMeta: Record<string, { id: string; baseUrl: string; model: string; keyRequired: boolean }> = {
  "Big Pickle": {
    id: "big-pickle",
    baseUrl: "",
    model: "opencode/big-pickle",
    keyRequired: true,
  },
  OpenAI: {
    id: "openai",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-5.6",
    keyRequired: true,
  },
  OpenRouter: {
    id: "openrouter",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "",
    keyRequired: true,
  },
  Ollama: {
    id: "ollama",
    baseUrl: "http://localhost:11434/v1",
    model: "",
    keyRequired: false,
  },
  Custom: {
    id: "custom",
    baseUrl: "",
    model: "",
    keyRequired: false,
  },
};

const sessionStorageKey = "boscode.session.current.messages";

const makeId = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;

export default function ChatWorkspace({
  provider,
  onProviderChange,
  onNotice,
  onOpenSettings,
  workspace,
  activeFile,
}: ChatWorkspaceProps) {
  const [messages, setMessages] = useState<Message[]>(() => {
    const raw = localStorage.getItem(sessionStorageKey);
    if (!raw) return [];

    try {
      const parsed = JSON.parse(raw) as Message[];
      return Array.isArray(parsed)
        ? parsed.map((message) => ({ ...message, streaming: false }))
        : [];
    } catch {
      return [];
    }
  });
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);

  const meta = providerMeta[provider] ?? providerMeta["Big Pickle"];

  const readProviderConfig = () => {
    const fallback = { baseUrl: meta.baseUrl, model: meta.model };
    const raw = localStorage.getItem(`boscode.provider.${meta.id}`);

    if (!raw) return fallback;

    try {
      return { ...fallback, ...JSON.parse(raw) } as {
        baseUrl: string;
        model: string;
      };
    } catch {
      return fallback;
    }
  };

  useEffect(() => {
    localStorage.setItem(
      sessionStorageKey,
      JSON.stringify(messages.map(({ streaming: _streaming, ...message }) => message)),
    );
  }, [messages]);

  const clearConversation = () => {
    setMessages([]);
    localStorage.removeItem(sessionStorageKey);
    onNotice("Conversation cleared");
  };

  const sendMessage = async () => {
    const prompt = input.trim();
    if (!prompt || sending) return;

    const memoryEnabled = localStorage.getItem("boscode.memory.enabled") !== "false";
    const autoCapture = localStorage.getItem("boscode.memory.autoCapture") !== "false";

    if (memoryEnabled && autoCapture) {
      try {
        const capture = await invoke<MemoryCaptureResult>("capture_memory_from_message", {
          content: prompt,
          workspace: workspace?.root ?? null,
        });

        if (capture.captured > 0) {
          onNotice(`BOSCode remembered ${capture.captured} preference${capture.captured === 1 ? "" : "s"}`);
        } else if (capture.skippedSensitive) {
          onNotice("Sensitive-looking content was not saved to memory");
        }
      } catch (caught) {
        console.warn("Memory capture unavailable", caught);
      }
    }

    const config = readProviderConfig();

    if (!config.model.trim() || (meta.id !== "big-pickle" && !config.baseUrl.trim())) {
      onNotice(
        meta.id === "big-pickle"
          ? `${provider} needs a model`
          : `${provider} needs a Base URL and model`,
      );
      onOpenSettings();
      return;
    }

    if (meta.keyRequired) {
      try {
        const hasKey = await invoke<boolean>("provider_secret_exists", {
          providerId: meta.id,
        });

        if (!hasKey) {
          onNotice(`${provider} needs an API key`);
          onOpenSettings();
          return;
        }
      } catch (caught) {
        onNotice(`Unable to read ${provider} credentials`);
        console.error(caught);
        return;
      }
    }

    const userMessage: Message = {
      id: makeId(),
      role: "user",
      content: prompt,
    };
    const assistantId = makeId();
    const assistantMessage: Message = {
      id: assistantId,
      role: "assistant",
      content: "",
      streaming: true,
    };
    let memoryContext = "";
    let workspaceContext = "";
    let terminalContext = "";

    if (memoryEnabled) {
      try {
        memoryContext = await invoke<string>("build_memory_context", {
          query: prompt,
          workspace: workspace?.root ?? null,
        });
      } catch (caught) {
        console.warn("Persistent memory unavailable", caught);
      }
    }

    if (workspace) {
      try {
        workspaceContext = await invoke<string>("build_workspace_context", {
          query: prompt,
          activeFile: activeFile?.path ?? null,
        });
      } catch (caught) {
        console.warn("Workspace context unavailable", caught);
      }

      try {
        terminalContext =
          (await invoke<string | null>("latest_command_context")) ?? "";
      } catch (caught) {
        console.warn("Terminal context unavailable", caught);
      }
    }

    const historyMessages = [...messages, userMessage].map(({ role, content }) => ({
      role,
      content,
    }));

    const agentContext = [memoryContext, workspaceContext, terminalContext]
      .filter(Boolean)
      .join("\n\n");

    const requestMessages = agentContext
      ? [
          {
            role: "system",
            content:
              "You are BOSCode, a coding agent working inside the user's selected repository. " +
              "Use persistent memory as user/project preferences and use the supplied workspace and approved-command context as evidence. Workspace memory overrides conflicting global memory. " +
              "Do not expose remembered context unnecessarily, and do not invent file contents, command results, or claim edits/commands were applied unless the context proves it. " +
              "If the latest approved command failed, analyze the failure and recommend the smallest concrete fix and the next command to validate it. " +
              "When proposing changes, name the exact files and explain what should change.\n\n" +
              agentContext,
          },
          ...historyMessages,
        ]
      : historyMessages;

    setInput("");
    setSending(true);
    setMessages((current) => [...current, userMessage, assistantMessage]);
    onNotice(`Sending to ${provider}…`);

    const onEvent = new Channel<StreamEvent>();
    onEvent.onmessage = (event) => {
      if (event.type === "started") {
        onNotice(`${provider} · ${event.model}`);
        return;
      }

      if (event.type === "delta") {
        setMessages((current) =>
          current.map((message) =>
            message.id === assistantId
              ? { ...message, content: message.content + event.content }
              : message,
          ),
        );
        return;
      }

      if (event.type === "completed") {
        setMessages((current) =>
          current.map((message) =>
            message.id === assistantId ? { ...message, streaming: false } : message,
          ),
        );
        onNotice(`${provider} response complete`);
      }

      if (event.type === "error") {
        setMessages((current) =>
          current.map((message) =>
            message.id === assistantId
              ? {
                  ...message,
                  streaming: false,
                  error: true,
                  content: message.content || event.message,
                }
              : message,
          ),
        );
        onNotice(`${provider} request failed`);
      }
    };

    try {
      await invoke("stream_chat", {
        providerId: meta.id,
        baseUrl: config.baseUrl.trim(),
        model: config.model.trim(),
        messages: requestMessages,
        onEvent,
      });
    } catch (caught) {
      const error = String(caught);
      setMessages((current) =>
        current.map((message) =>
          message.id === assistantId
            ? {
                ...message,
                streaming: false,
                error: true,
                content: message.content || error,
              }
            : message,
        ),
      );
      onNotice(`${provider} request failed`);
    } finally {
      setSending(false);
    }
  };

  return (
    <Fragment>
      <div className="chat-panel">
        {messages.length === 0 ? (
          <div className="empty-chat">
            <div className="empty-chat-mark">◇</div>
            <h2>What should we build?</h2>
            <p>
              BOSCode can now send this conversation through your selected AI provider.
              Configure the provider key once, then work from here.
            </p>
            <button onClick={onOpenSettings}>Configure {provider}</button>
          </div>
        ) : (
          <>
            <div className="chat-session-actions">
              <span>{messages.length} messages</span>
              <button onClick={clearConversation} disabled={sending}>
                Clear
              </button>
            </div>

            {messages.map((message) => (
              <article
                className={`message ${message.role === "user" ? "user-message" : "agent-message"} ${message.error ? "message-error" : ""}`}
                key={message.id}
              >
                <div className={`message-avatar ${message.role === "user" ? "user" : "agent"}`}>
                  {message.role === "user" ? "H" : "◇"}
                </div>
                <div>
                  <div className="agent-title">
                    <strong>{message.role === "user" ? "You" : "BOSCode"}</strong>
                    {message.role === "assistant" && <span>{provider}</span>}
                    {message.streaming && <i />}
                  </div>
                  <p className="chat-content">
                    {message.content || (message.streaming ? "Thinking…" : "")}
                    {message.streaming && message.content && <span className="streaming-cursor" />}
                  </p>
                </div>
              </article>
            ))}
          </>
        )}
      </div>

      {workspace && (
        <div className="context-strip">
          <span>▱ {workspace.name}</span>
          <span>⑂ {workspace.branch ?? "no branch"}</span>
          {activeFile && <span className="active-context-file">＋ {activeFile.path}</span>}
          <small>memory + repo + approved command context</small>
        </div>
      )}

      <div className="composer">
        <textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void sendMessage();
            }
          }}
          placeholder={sending ? "BOSCode is responding…" : "Ask BOSCode anything…"}
          rows={1}
          disabled={sending}
        />
        <div className="composer-actions">
          <select
            value={provider}
            onChange={(event) => onProviderChange(event.target.value)}
            disabled={sending}
          >
            <option>Big Pickle</option>
            <option>OpenAI</option>
            <option>OpenRouter</option>
            <option>Ollama</option>
            <option>Custom</option>
          </select>
          <button className="attach" title="Provider settings" onClick={onOpenSettings}>
            ⚙
          </button>
          <button
            className="send"
            onClick={() => void sendMessage()}
            aria-label="Send"
            disabled={sending || !input.trim()}
          >
            {sending ? "…" : "➜"}
          </button>
        </div>
      </div>
    </Fragment>
  );
}
