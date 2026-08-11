import { useCallback, useEffect, useState, type FormEvent } from "react";
import { command, failureMessage } from "./api";
import { Field, InlineNotice, Loading, Panel, valueOf } from "./components";
import { ExperimentsPage } from "./pages/ExperimentsPage";
import { ExplorePage } from "./pages/ExplorePage";
import { PrivacyPage } from "./pages/PrivacyPage";
import { ReflogPage } from "./pages/ReflogPage";
import { ReviewPage } from "./pages/ReviewPage";
import { SelfMapPage } from "./pages/SelfMapPage";
import { TimelinePage } from "./pages/TimelinePage";
import type { AppStatus } from "./types";

type Page = "explore" | "self-map" | "review" | "timeline" | "reflog" | "experiments" | "privacy";

const navigation: Array<{ id: Page; label: string; glyph: string }> = [
  { id: "explore", label: "Explore", glyph: "◌" },
  { id: "self-map", label: "Self Map", glyph: "◇" },
  { id: "review", label: "Review", glyph: "✓" },
  { id: "timeline", label: "Timeline", glyph: "│" },
  { id: "reflog", label: "Reflog", glyph: "↺" },
  { id: "experiments", label: "Experiments", glyph: "△" },
  { id: "privacy", label: "Privacy & Models", glyph: "⌾" },
];

export default function App() {
  const [status, setStatus] = useState<AppStatus>();
  const [page, setPage] = useState<Page>("explore");
  const [error, setError] = useState<string>();
  const [banner, setBanner] = useState<string>();

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await command<AppStatus>("app_status"));
      setError(undefined);
    } catch (cause) {
      setError(failureMessage(cause));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  async function changed(notice?: string) {
    setBanner(notice);
    await refreshStatus();
  }

  if (status === undefined) {
    return <Splash error={error} />;
  }
  if (!status.vaultExists) {
    return <Onboarding status={status} onChanged={changed} />;
  }
  if (!status.unlocked) {
    return <UnlockGate status={status} onChanged={changed} />;
  }

  async function lock() {
    try {
      await command("vault_lock");
    } finally {
      await refreshStatus();
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">Q</div>
          <div><strong>Quisum</strong><span>Evidence, not essence</span></div>
        </div>
        <nav aria-label="Primary navigation">
          {navigation.map((item) => (
            <button
              className={page === item.id ? "nav-item nav-item-active" : "nav-item"}
              key={item.id}
              onClick={() => setPage(item.id)}
              aria-current={page === item.id ? "page" : undefined}
            >
              <span aria-hidden="true">{item.glyph}</span>{item.label}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <div className="vault-state"><span className="status-dot" />Encrypted vault unlocked</div>
          <small>Local-first · no telemetry</small>
          <small>Development build {status.version}</small>
        </div>
      </aside>
      <main className="main-content">
        {banner !== undefined && (
          <div className="global-banner">
            <InlineNotice tone="warning">{banner}</InlineNotice>
            <button className="text-button" onClick={() => setBanner(undefined)}>Dismiss</button>
          </div>
        )}
        {page === "explore" && <ExplorePage />}
        {page === "self-map" && <SelfMapPage />}
        {page === "review" && <ReviewPage />}
        {page === "timeline" && <TimelinePage />}
        {page === "reflog" && <ReflogPage />}
        {page === "experiments" && <ExperimentsPage />}
        {page === "privacy" && <PrivacyPage status={status} onLocked={lock} />}
      </main>
    </div>
  );
}

function Splash({ error }: { error?: string }) {
  return (
    <main className="gate-shell">
      <div className="gate-brand"><div className="brand-mark brand-mark-large">Q</div><h1>Quisum</h1></div>
      {error === undefined ? <Loading /> : <InlineNotice tone="danger">{error}</InlineNotice>}
    </main>
  );
}

export function Onboarding({ status, onChanged }: { status: AppStatus; onChanged: (notice?: string) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const recoveryPassphrase = valueOf(form, "recoveryPassphrase");
    const recoveryConfirmation = valueOf(form, "recoveryConfirmation");
    clearSecretFields(target, ["recoveryPassphrase", "recoveryConfirmation"]);
    setBusy(true);
    try {
      const saveToKeyring = form.has("saveToKeyring");
      const result = await command<{ keyringSaved: boolean }>("vault_create", {
        recoveryPassphrase,
        recoveryConfirmation,
        saveToKeyring,
      });
      await onChanged(saveToKeyring && !result.keyringSaved
        ? "The vault was created, but the OS credential store did not save its key. Keep the recovery passphrase: it will be required after restart."
        : undefined);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      clearSecretFields(target, ["recoveryPassphrase", "recoveryConfirmation"]);
      setBusy(false);
    }
  }

  async function restore(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const recoveryPassphrase = valueOf(form, "restorePassphrase");
    clearSecretFields(target, ["restorePassphrase"]);
    setBusy(true);
    try {
      const saveToKeyring = form.has("saveRestoredKey");
      const result = await command<{ keyringSaved: boolean }>("vault_restore_backup", {
        source: valueOf(form, "source"),
        recoveryPassphrase,
        saveToKeyring,
      });
      await onChanged(saveToKeyring && !result.keyringSaved
        ? "The backup was restored, but the OS credential store did not save its key. Keep the recovery passphrase: it will be required after restart."
        : undefined);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      clearSecretFields(target, ["restorePassphrase"]);
      setBusy(false);
    }
  }

  return (
    <main className="gate-shell onboarding-shell">
      <header className="onboarding-header">
        <div className="gate-brand"><div className="brand-mark brand-mark-large">Q</div><h1>Quisum</h1></div>
        <p>A private place to observe, test, revise, and remain the final authority on what your records mean.</p>
      </header>
      {notice !== undefined && <InlineNotice tone="danger">{notice}</InlineNotice>}
      <div className="gate-grid">
        <Panel title="Create an encrypted vault">
          <p className="secondary-copy">A random SQLCipher key protects the vault. Your passphrase wraps that key for recovery; Quisum cannot recover it for you.</p>
          <form className="form-stack" onSubmit={create}>
            <Field label="Recovery passphrase" hint="At least 12 characters; use a unique passphrase.">
              <input name="recoveryPassphrase" type="password" required minLength={12} maxLength={1024} autoComplete="new-password" autoFocus />
            </Field>
            <Field label="Repeat recovery passphrase">
              <input name="recoveryConfirmation" type="password" required minLength={12} maxLength={1024} autoComplete="new-password" />
            </Field>
            <label className="check-confirm"><input type="checkbox" name="saveToKeyring" defaultChecked /> Save the random vault key in the operating-system credential store.</label>
            <button className="button button-primary button-large" disabled={busy} type="submit">{busy ? "Creating…" : "Create local vault"}</button>
          </form>
          <div className="path-note"><span>Vault location</span><code>{status.vaultPath}</code></div>
          <div className="path-note"><span>Recovery envelope</span><code>{status.recoveryPath}</code></div>
        </Panel>
        <Panel title="Restore an encrypted backup">
          <p className="secondary-copy">Restore a verified <code>.qsb</code> backup into this fresh application-data location. Existing files are never replaced.</p>
          <form className="form-stack" onSubmit={restore}>
            <Field label="Existing backup path"><input name="source" required maxLength={4096} placeholder="/path/to/backup.qsb" /></Field>
            <Field label="Backup recovery passphrase">
              <input name="restorePassphrase" type="password" required minLength={12} maxLength={1024} autoComplete="current-password" />
            </Field>
            <label className="check-confirm"><input type="checkbox" name="saveRestoredKey" defaultChecked /> Save the restored random key in the OS credential store.</label>
            <button className="button button-large" disabled={busy} type="submit">Restore and verify</button>
          </form>
        </Panel>
      </div>
      <p className="gate-disclaimer">Quisum is a self-inquiry tool, not diagnosis, treatment, emergency response, or a substitute for human relationships and professional care.</p>
    </main>
  );
}

export function UnlockGate({ status, onChanged }: { status: AppStatus; onChanged: (notice?: string) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();

  async function unlockKeyring() {
    setBusy(true);
    try {
      await command("vault_unlock_keyring");
      await onChanged();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function unlockRecovery(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const recoveryPassphrase = valueOf(form, "recoveryPassphrase");
    clearSecretFields(target, ["recoveryPassphrase"]);
    setBusy(true);
    try {
      const saveToKeyring = form.has("saveToKeyring");
      const result = await command<{ keyringSaved: boolean }>("vault_unlock_recovery", {
        recoveryPassphrase,
        saveToKeyring,
      });
      await onChanged(saveToKeyring && !result.keyringSaved
        ? "The vault is unlocked, but the OS credential store did not save its key. The recovery passphrase will be required again after restart."
        : undefined);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      clearSecretFields(target, ["recoveryPassphrase"]);
      setBusy(false);
    }
  }

  return (
    <main className="gate-shell unlock-shell">
      <div className="gate-brand"><div className="brand-mark brand-mark-large">Q</div><h1>Unlock Quisum</h1></div>
      <p className="gate-lede">Your encrypted self-inquiry workspace stays local to this device.</p>
      {notice !== undefined && <InlineNotice tone="danger">{notice}</InlineNotice>}
      <Panel className="unlock-panel">
        <button className="button button-primary button-large" disabled={busy} onClick={() => void unlockKeyring()}>{busy ? "Unlocking…" : "Unlock with OS credential store"}</button>
        <div className="or-divider"><span>or recover manually</span></div>
        <form className="form-stack" onSubmit={unlockRecovery}>
          <Field label="Recovery passphrase">
            <input name="recoveryPassphrase" type="password" required minLength={12} maxLength={1024} autoComplete="current-password" />
          </Field>
          <label className="check-confirm"><input type="checkbox" name="saveToKeyring" /> Save the recovered key to the OS credential store.</label>
          <button className="button" disabled={busy || !status.recoveryExists} type="submit">Unlock with recovery envelope</button>
        </form>
        {!status.recoveryExists && <InlineNotice tone="danger">The expected local recovery envelope is missing.</InlineNotice>}
      </Panel>
      <div className="path-note"><span>Encrypted vault</span><code>{status.vaultPath}</code></div>
    </main>
  );
}

function clearSecretFields(form: HTMLFormElement, names: string[]) {
  for (const name of names) {
    const field = form.elements.namedItem(name);
    if (field instanceof HTMLInputElement) field.value = "";
  }
}
