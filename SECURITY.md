# BOSCode Security

BOSCode is a local-first coding agent with access to source code, approved workspace writes, approved commands, Git, and AI providers. Security boundaries are treated as product features.

## Core protections

- API keys are stored through the operating system credential store.
- Sensitive files such as `.env`, private keys, certificates, and common credential files are excluded from AI repository context and blocked from BOSCode staging.
- Workspace file writes require explicit Workspace Write permission and reviewed patches.
- Commands are staged and explicitly approved before execution.
- Normal terminal commands execute as executable + argument arrays instead of unrestricted shell strings.
- Git mutations and network operations are staged for explicit approval.
- Force push, hard reset, and branch deletion are not exposed.
- Production builds use a restrictive WebView content security policy.
- In-app updater packages are accepted only when signed by the configured Tauri updater key.
- Production logs rotate in the platform application log directory.

## Reporting

Do not publish credentials, tokens, private keys, customer source code, or exploit details in a public issue. Contact the BusinessOS/BOSCode maintainer privately when reporting a security issue.
