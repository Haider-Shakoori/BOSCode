import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { useEffect, useState } from "react";

type UpdateCenterProps = {
  onClose: () => void;
  onNotice: (message: string) => void;
};

type UpdateInfo = {
  version: string;
  date: string | null;
  body: string | null;
};

export default function UpdateCenter({ onClose, onNotice }: UpdateCenterProps) {
  const [currentVersion, setCurrentVersion] = useState("1.0.0");
  const [available, setAvailable] = useState<UpdateInfo | null>(null);
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [checked, setChecked] = useState(false);

  useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => undefined);
  }, []);

  const checkForUpdates = async () => {
    setBusy("check");
    setError("");
    setProgress(null);

    try {
      const update = await check();
      setChecked(true);

      if (!update) {
        setAvailable(null);
        onNotice("BOSCode is up to date");
        return;
      }

      setAvailable({
        version: update.version,
        date: update.date ?? null,
        body: update.body ?? null,
      });
      await update.close();
      onNotice(`BOSCode ${update.version} is available`);
    } catch (caught) {
      const message = String(caught);
      setError(
        message.includes("BOSCODE_UPDATER_PUBKEY") || message.toLowerCase().includes("public key")
          ? "Signed updates are not configured for this developer build."
          : message,
      );
      onNotice("Update check failed");
    } finally {
      setBusy(null);
    }
  };

  const installUpdate = async () => {
    setBusy("install");
    setError("");
    setProgress(0);

    try {
      const update = await check();
      if (!update) {
        setAvailable(null);
        setChecked(true);
        onNotice("BOSCode is already up to date");
        return;
      }

      let downloaded = 0;
      let total: number | null = null;

      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          setProgress(0);
          return;
        }

        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total && total > 0) {
            setProgress(Math.min(100, Math.round((downloaded / total) * 100)));
          }
          return;
        }

        if (event.event === "Finished") {
          setProgress(100);
        }
      });

      onNotice("Update downloaded — Windows installer is starting");
    } catch (caught) {
      setError(String(caught));
      onNotice("Update installation failed");
      setBusy(null);
    }
  };

  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="update-window"
        role="dialog"
        aria-modal="true"
        aria-label="BOSCode Update Center"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="update-titlebar">
          <div>
            <span className="update-logo">◇</span>
            <div>
              <strong>BOSCode Update Center</strong>
              <span>Signed releases delivered through GitHub</span>
            </div>
          </div>
          <button onClick={onClose} aria-label="Close Update Center">×</button>
        </header>

        <div className="update-content">
          <div className="update-version-card">
            <div>
              <small>INSTALLED VERSION</small>
              <strong>v{currentVersion}</strong>
              <span>Stable channel · Windows</span>
            </div>
            <span className="update-shield">✓ Signed updater</span>
          </div>

          {available ? (
            <div className="update-available">
              <span className="update-orbit">↻</span>
              <div>
                <small>UPDATE AVAILABLE</small>
                <h2>BOSCode v{available.version}</h2>
                {available.date && <span>{available.date}</span>}
                <p>{available.body || "A new signed BOSCode release is ready to install."}</p>
              </div>
            </div>
          ) : (
            <div className="update-state">
              <span>{checked && !error ? "✓" : "↻"}</span>
              <strong>
                {checked && !error ? "You’re up to date" : "Check for the latest BOSCode release"}
              </strong>
              <p>
                Update metadata and installer signatures are verified before installation.
              </p>
            </div>
          )}

          {progress !== null && (
            <div className="update-progress">
              <div>
                <span>Downloading update</span>
                <strong>{progress}%</strong>
              </div>
              <div className="update-progress-track">
                <span style={{ width: `${progress}%` }} />
              </div>
            </div>
          )}

          {error && (
            <div className="update-error">
              <strong>Update unavailable</strong>
              <span>{error}</span>
            </div>
          )}

          <div className="update-security">
            <strong>Release security</strong>
            <p>
              BOSCode only installs updater bundles that match the embedded Tauri signing
              public key. The private updater key stays in GitHub Actions secrets and is
              never shipped inside the desktop application.
            </p>
          </div>
        </div>

        <footer className="update-actions">
          <button className="secondary-button" onClick={onClose} disabled={busy === "install"}>
            Close
          </button>
          {available ? (
            <button
              className="primary-button"
              onClick={() => void installUpdate()}
              disabled={busy !== null}
            >
              {busy === "install" ? "Installing…" : `Download & Install v${available.version}`}
            </button>
          ) : (
            <button
              className="primary-button"
              onClick={() => void checkForUpdates()}
              disabled={busy !== null}
            >
              {busy === "check" ? "Checking…" : "Check for Updates"}
            </button>
          )}
        </footer>
      </section>
    </div>
  );
}
