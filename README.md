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
- [x] Batch 5 — reviewed file changes, diffs, apply/reject/undo, and write permissions
- [x] Batch 6 — approved terminal execution, project checks, and AI command-result context
- [x] Batch 7 — approval-gated Git/GitHub workflows and PR assistance

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

### Batch 6 — approved terminal execution

- [x] Explicit command staging and Run approval
- [x] Workspace-scoped process execution
- [x] Live stdout/stderr streaming to the Tauri UI
- [x] Cancel active command
- [x] Command history and last-run status
- [x] Automatic project check detection for npm, Laravel, Composer, Rust, Flutter, and pytest
- [x] Latest approved command output included in the next AI context
- [x] Shell-free executable/argument parsing for normal commands
- [x] Safe Windows .cmd/.bat shim handling for tools such as npm
- [x] Regression tests for parsing and command safety

### Batch 7 — Git and GitHub workflows

- [x] Git repository status, staged/unstaged/untracked files
- [x] Branch, upstream, ahead/behind, remotes, and recent commits
- [x] Approval-gated staging and unstaging
- [x] Approval-gated commits
- [x] Approval-gated branch creation and switching
- [x] Fast-forward-only pull
- [x] Push and safe upstream setup without force push
- [x] GitHub remote detection
- [x] GitHub CLI installation/authentication status
- [x] Pull request draft assistance from recent repository history
- [x] Approval-gated GitHub pull request creation through authenticated `gh`
- [x] Sensitive-file staging and commit protection
- [x] No force push, hard reset, or branch deletion workflows

### Batch 8 — production release

- [x] BOSCode v1.0.0 version alignment
- [x] Restrictive WebView content security policy
- [x] Rotating production logs
- [x] Signed GitHub Release updater configuration
- [x] In-app Update Center with download/install progress
- [x] Windows NSIS setup executable configuration
- [x] Windows MSI installer configuration
- [x] Installer downgrade protection
- [x] Release version consistency check
- [x] Rust formatting and Clippy gates
- [x] Full Windows installer smoke build in CI
- [x] GitHub Release workflow
- [x] Signed updater artifact support when repository signing secrets are configured
- [x] Security policy and release documentation

### Roadmap status

**Batches 1–8 complete.** BOSCode v1.0.0 is the first production-release milestone.

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
cargo test --manifest-path src-tauri/Cargo.toml
npm run version:check
```

### Build the Windows installer

```powershell
npm run tauri build
```

This produces the NSIS setup executable and MSI bundle under `src-tauri/target/release/bundle/`.

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

BOSCode does not require a BOSCode-owned live server for the core desktop experience. AI traffic goes directly to the provider configured by the user, and release updates can be delivered through signed GitHub Release metadata.

See [SECURITY.md](SECURITY.md) for the security model and [docs/RELEASING.md](docs/RELEASING.md) for the production release process.


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
