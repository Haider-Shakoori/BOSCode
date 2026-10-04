# BOSCode

**AI Coding Agent by BusinessOS**

BOSCode is a local-first desktop coding agent designed to understand a repository, plan work, edit files, run commands, inspect diffs, test changes, and work with Git from one premium desktop workspace.

## Stack

- Tauri 2 / Rust
- React 19 + TypeScript
- Vite
- Local-first desktop architecture
- Provider abstraction for Big Pickle, OpenAI, Anthropic, Gemini, OpenRouter, Ollama, and custom OpenAI-compatible endpoints

## Current milestone

### Batch 1 — Desktop foundation

- [x] Tauri 2 Windows desktop shell
- [x] React + TypeScript frontend
- [x] Premium BOSCode workspace UI
- [x] Chat, sessions, task progress, diff and terminal surfaces
- [x] Provider selector shell
- [x] Rust command foundation
- [x] Windows GitHub Actions validation
- [x] Real project/folder access
- [ ] Secure API-key storage
- [ ] Streaming AI provider connection
- [ ] Agent file write operations
- [ ] Terminal execution
- [ ] Git operations

## Local development

### Prerequisites

- Node.js 24+
- Rust stable (MSVC toolchain on Windows)
- Microsoft C++ Build Tools
- Microsoft Edge WebView2
- Git

### Run the frontend

```powershell
npm install
npm run dev
```

### Run BOSCode as a desktop app

```powershell
npm install
npm run tauri dev
```

### Validate

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

## Architecture direction

```text
BOSCode Desktop
├── React + TypeScript UI
├── Tauri IPC
├── Rust Agent Runtime
│   ├── Workspace / filesystem
│   ├── Terminal / processes
│   ├── Git
│   ├── Patch / diff engine
│   ├── Permission layer
│   └── Provider abstraction
├── Secure OS credential storage
└── SQLite metadata / sessions
```

BOSCode is being built so no BOSCode-owned live server is required for the core desktop experience.


### Batch 4 — Workspace intelligence

- Native folder selection
- Sandboxed workspace root
- Read-only repository indexing
- File explorer and text preview
- Repository-wide text search
- Basic Git branch detection
- Sensitive file filtering
- Prompt-aware repository context for AI chat
- Rust regression tests for workspace safety

The workspace layer is intentionally read-only at this stage. Write operations and process execution will require explicit permission controls in the next agent-tools batch.
