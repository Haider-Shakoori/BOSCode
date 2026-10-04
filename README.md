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

### Completed

- [x] Batch 1 — Tauri 2 desktop foundation and premium workspace UI
- [x] Batch 2 — secure AI provider settings and OS credential storage
- [x] Batch 3 — real streaming AI chat and local session history
- [x] Batch 4 — workspace selection, repository indexing, safe reads, search, and AI context

### Batch 5 — reviewed agent changes

- [x] Read-only permission mode by default
- [x] Explicit Workspace Write permission
- [x] Propose file create/update/delete operations
- [x] Unified diff previews
- [x] Apply / Reject / Apply All
- [x] Undo Last
- [x] Stale-file protection before apply and undo
- [x] Sensitive-file and workspace-boundary protection
- [x] Explorer editor and new-file proposal UI
- [x] Rust regression tests for permission, paths, and diffs

### Remaining roadmap

- [ ] Batch 6 — terminal execution, command approval, and build/test/fix loops
- [ ] Batch 7 — Git/GitHub workflows, branches, commits, push/pull, and PR assistance
- [ ] Batch 8 — production hardening, updater, signed installer pipeline, regression suite

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
