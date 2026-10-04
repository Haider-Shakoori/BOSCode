# Releasing BOSCode

BOSCode releases are built on Windows through `.github/workflows/release.yml`.

## Versioning

The version must match in:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

Run:

```powershell
npm run version:check
```

## Updater signing

Generate a Tauri updater key pair on a trusted machine:

```powershell
npm run tauri signer generate -w $HOME\.tauri\boscode.key
```

Store the private material only in GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when the private key has a password
- `BOSCODE_UPDATER_PUBKEY` containing the matching public key

The release workflow injects the public key at build time and enables updater artifact generation. The private key is never committed to the repository.

Without these secrets, the workflow still produces Windows NSIS and MSI installer artifacts, but signed in-app update metadata is not generated.

## Release

Either push a version tag such as `v1.0.0` or run **BOSCode Release** manually from GitHub Actions.

The workflow runs the production regression suite and publishes:

- Windows NSIS setup executable
- Windows MSI installer
- GitHub Release assets
- `latest.json` and updater signatures when updater signing secrets are configured
- A 30-day GitHub Actions copy of the Windows bundle artifacts

## Windows Authenticode

Tauri updater signing verifies update bundles but is separate from Windows Authenticode signing. Add a trusted Windows code-signing certificate before public commercial distribution to reduce SmartScreen warnings.
