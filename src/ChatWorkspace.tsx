import { Channel, invoke } from "@tauri-apps/api/core";
import { Fragment, useEffect, useMemo, useState } from "react";

type ChatWorkspaceProps = {
  provider: string;
  onProviderChange: (provider: string) => void;
  onNotice: (message: string) => void;
  onOpenSettings: () => void;
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

const providerMeta: Record<string, { id: string; baseUrl: string; model: string }> = {
  "Big Pickle": {
    id: "big-pickle",
    baseUrl: "https://opencode.ai/zen/v1",
    model: "big-pickle",
  },
  OpenAI: {
    id: "openai",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-5.6",
  },
  OpenRouter: {
    id: "openrouter",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "",
  },
  Ollama: {
    id: "ollama",
    baseUrl: "http://localhost:11434/v1",
    model: "",
  },
  Custom: {
    id: "custom",
    baseUrl: "",
    model: "",
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
  const config = useMemo(() => {
    const raw = localStorage.getItem(`boscode.provider.${meta.id}`);
    const fallback = { baseUrl: meta.baseUrl, model: meta.model };

    if (!raw) return fallback;

    try {
      return { ...fallback, ...JSON.parse(raw) } as {
        baseUrl: string;
        model: string;
      };
    } catch {
      return fallback;
    }
  }, [meta.baseUrl, meta.id, meta.model]);

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

    if (!config.baseUrl.trim() || !config.model.trim()) {
      onNotice(`${provider} needs a Base URL and model`);
      onOpenSettings();
      return;
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
    const requestMessages = [...messages, userMessage].map(({ role, content }) => ({
      role,
      content,
    }));

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
