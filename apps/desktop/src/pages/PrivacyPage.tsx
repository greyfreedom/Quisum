import { useCallback, useEffect, useState, type FormEvent } from "react";
import { command, failureMessage } from "../api";
import {
  EmptyState,
  Field,
  InlineNotice,
  Loading,
  PageHeader,
  Panel,
  SensitivityPill,
  StatusPill,
  formatDate,
  valueOf,
} from "../components";
import type { AppStatus, MethodSummary, ProviderProfile } from "../types";

export function PrivacyPage({ status, onLocked }: { status: AppStatus; onLocked: () => Promise<void> }) {
  const [providers, setProviders] = useState<ProviderProfile[]>([]);
  const [methods, setMethods] = useState<MethodSummary[]>([]);
  const [kind, setKind] = useState("openai");
  const [privacy, setPrivacy] = useState("cloud");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [revokeTarget, setRevokeTarget] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [nextProviders, nextMethods] = await Promise.all([
        command<ProviderProfile[]>("provider_list"),
        command<MethodSummary[]>("method_list"),
      ]);
      setProviders(nextProviders);
      setMethods(nextMethods);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function createProvider(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const credential = valueOf(form, "credential");
    clearSecretFields(target, ["credential"]);
    setBusy(true);
    try {
      await command<ProviderProfile>("provider_create", {
        name: valueOf(form, "name"),
        kind: valueOf(form, "kind"),
        baseUrl: valueOf(form, "baseUrl"),
        model: valueOf(form, "model"),
        privacyKind: valueOf(form, "privacyKind"),
        trustedSensitivities: form.getAll("trustedSensitivities").map(String),
        streaming: form.has("streaming"),
        toolUse: form.has("toolUse"),
        jsonSchema: form.has("jsonSchema"),
        vision: form.has("vision"),
        contextLimit: valueOf(form, "contextLimit") === "" ? null : Number(valueOf(form, "contextLimit")),
        retentionNote: valueOf(form, "retentionNote") || null,
        credential,
      });
      target.reset();
      setKind("openai");
      setPrivacy("cloud");
      setNotice("Provider metadata saved in the vault; the credential was stored separately by the operating system.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      clearSecretFields(target, ["credential"]);
      setBusy(false);
    }
  }

  async function testProvider(id: string) {
    setBusy(true);
    try {
      const result = await command<{ succeeded: boolean; latencyMs: number }>("provider_test", { id });
      setNotice(result.succeeded ? `Connection succeeded in ${result.latencyMs} ms without sending personal context.` : "Connection test failed.");
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function toggleProvider(profile: ProviderProfile) {
    setBusy(true);
    try {
      await command("provider_toggle", { id: profile.id, enabled: !profile.enabled });
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function revoke(id: string) {
    setBusy(true);
    try {
      await command("provider_revoke", { id });
      setRevokeTarget(undefined);
      setNotice("The credential was deleted from the operating-system store and the profile was disabled.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function lockVault() {
    setBusy(true);
    try {
      await onLocked();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function exportData(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    try {
      await command("data_export", {
        destination: valueOf(form, "destination"),
        format: valueOf(form, "format"),
        acknowledgePlaintext: form.has("acknowledgePlaintext"),
      });
      setNotice("Portable export written and verified. Treat it as plaintext, not as an encrypted backup.");
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function importData(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    try {
      await command("data_import", {
        source: valueOf(form, "source"),
        confirmEmptyVaultImport: form.has("confirmEmptyVaultImport"),
      });
      setNotice("Portable graph imported atomically. Imported provider profiles remain disabled and have no inherited credential authority.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function backup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const recoveryPassphrase = valueOf(form, "backupPassphrase");
    const recoveryConfirmation = valueOf(form, "backupConfirmation");
    clearSecretFields(target, ["backupPassphrase", "backupConfirmation"]);
    setBusy(true);
    try {
      await command("data_backup", {
        destination: valueOf(form, "destination"),
        recoveryPassphrase,
        recoveryConfirmation,
      });
      setNotice("Encrypted backup written, authenticated, and re-opened for verification.");
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      clearSecretFields(target, ["backupPassphrase", "backupConfirmation"]);
      setBusy(false);
    }
  }

  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Custody and least disclosure"
        title="Privacy & Models"
        description="Provider profiles are replaceable policy records. Keys remain in the operating-system credential store, and only Rust performs network requests."
        action={<button className="button" disabled={busy} onClick={() => void lockVault()}>Lock vault</button>}
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}

      <div className="privacy-summary">
        <div><span>Vault</span><strong>SQLCipher encrypted</strong><small>{status.vaultPath}</small></div>
        <div><span>Recovery</span><strong>{status.recoveryExists ? "Envelope present" : "Missing"}</strong><small>{status.recoveryPath}</small></div>
        <div><span>Telemetry</span><strong>None</strong><small>No account, sync, analytics, or crash upload</small></div>
      </div>

      <Panel title="Configured providers" aside={<span className="count-badge">{providers.length}</span>}>
        {loading ? (
          <Loading />
        ) : providers.length === 0 ? (
          <EmptyState>No model provider is configured. All local capture and review features still work.</EmptyState>
        ) : (
          <div className="provider-grid">
            {providers.map((profile) => (
              <article className="provider-card" key={profile.id}>
                <div className="record-card-header">
                  <div><h3>{profile.name}</h3><small>{profile.model}</small></div>
                  <div className="item-meta"><StatusPill value={profile.enabled ? "enabled" : "disabled"} /><StatusPill value={profile.privacy_kind} /></div>
                </div>
                <dl className="compact-facts">
                  <div><dt>Protocol</dt><dd>{profile.kind}</dd></div>
                  <div><dt>Endpoint</dt><dd className="break-all">{profile.base_url}</dd></div>
                  <div><dt>Credential</dt><dd>{profile.credential_ref === undefined ? "none" : "stored separately"}</dd></div>
                  <div><dt>Updated</dt><dd>{formatDate(profile.updated_at)}</dd></div>
                </dl>
                <div className="item-meta sensitivity-row">
                  {profile.trusted_sensitivities.map((value) => <SensitivityPill key={value} value={value} />)}
                </div>
                {profile.retention_note !== undefined && <p className="secondary-copy"><strong>Retention note:</strong> {profile.retention_note}</p>}
                <div className="button-row">
                  <button className="button" disabled={busy || !profile.enabled} onClick={() => void testProvider(profile.id)}>Test connection</button>
                  <button className="button" disabled={busy} onClick={() => void toggleProvider(profile)}>{profile.enabled ? "Disable" : "Enable"}</button>
                </div>
                {profile.credential_ref !== undefined && (
                  revokeTarget === profile.id ? (
                    <div className="danger-confirm">
                      <p>Delete this credential and disable new calls? Historical provenance remains.</p>
                      <div className="button-row">
                        <button className="button button-danger" disabled={busy} onClick={() => void revoke(profile.id)}>Confirm revoke</button>
                        <button className="button" onClick={() => setRevokeTarget(undefined)}>Cancel</button>
                      </div>
                    </div>
                  ) : <button className="text-button danger-text" onClick={() => setRevokeTarget(profile.id)}>Revoke credential</button>
                )}
              </article>
            ))}
          </div>
        )}
      </Panel>

      <Panel title="Add a provider">
        <form className="form-stack" onSubmit={createProvider}>
          <div className="form-grid three">
            <Field label="Name"><input name="name" required maxLength={120} /></Field>
            <Field label="Protocol">
              <select name="kind" value={kind} onChange={(event) => {
                const next = event.target.value;
                setKind(next);
                setPrivacy(next === "local_http" ? "local" : "cloud");
              }}>
                <option value="openai">OpenAI Responses</option>
                <option value="anthropic">Anthropic Messages</option>
                <option value="openai_compatible">OpenAI-compatible</option>
                <option value="local_http">Local loopback HTTP</option>
              </select>
            </Field>
            <Field label="Boundary">
              <input name="privacyKind" type="hidden" value={privacy} />
              <select value={privacy} onChange={(event) => setPrivacy(event.target.value)} disabled={kind === "openai" || kind === "anthropic" || kind === "local_http"}>
                <option value="cloud">Cloud</option>
                <option value="local">Local device</option>
              </select>
            </Field>
          </div>
          <div className="form-grid">
            <Field label="Base URL" hint={privacy === "cloud" ? "HTTPS and no embedded credentials" : "Loopback address only"}>
              <input name="baseUrl" type="url" required maxLength={1000} placeholder={kind === "local_http" ? "http://127.0.0.1:11434/v1" : "https://api.example.com/v1"} />
            </Field>
            <Field label="Model identifier"><input name="model" required maxLength={160} /></Field>
          </div>
          <Field label="API credential" hint={privacy === "cloud" ? "Required. Sent directly to the OS credential store; never saved in frontend state or the vault." : "Optional for authenticated loopback endpoints."}>
            <input name="credential" type="password" required={privacy === "cloud"} maxLength={16384} autoComplete="off" spellCheck={false} />
          </Field>
          <fieldset>
            <legend>Allowed outbound sensitivity</legend>
            <div className="checkbox-row">
              <label><input type="checkbox" name="trustedSensitivities" value="normal" defaultChecked /> Normal</label>
              <label><input type="checkbox" name="trustedSensitivities" value="private" defaultChecked /> Private</label>
              <label><input type="checkbox" name="trustedSensitivities" value="highly_sensitive" /> Highly sensitive, with one-time packet approval</label>
            </div>
          </fieldset>
          <fieldset>
            <legend>Declared capabilities</legend>
            <div className="checkbox-row">
              <label><input type="checkbox" name="streaming" defaultChecked /> Streaming</label>
              <label><input type="checkbox" name="jsonSchema" defaultChecked /> JSON Schema</label>
              <label><input type="checkbox" name="toolUse" /> Tool use</label>
              <label><input type="checkbox" name="vision" /> Vision</label>
            </div>
          </fieldset>
          <div className="form-grid">
            <Field label="Declared context limit"><input name="contextLimit" type="number" min={1} max={100000000} /></Field>
            <Field label="Provider retention note"><input name="retentionNote" maxLength={2000} /></Field>
          </div>
          <button className="button button-primary" disabled={busy} type="submit">Add provider</button>
        </form>
      </Panel>

      <Panel title="Official method packages">
        <InlineNotice tone="warning">Review labels are factual metadata. These bundled V1 methods currently remain unreviewed and are not presented as clinical care.</InlineNotice>
        <div className="method-grid">
          {methods.map((method) => (
            <article key={method.id}>
              <div className="record-card-header"><h3>{method.title}</h3><StatusPill value={method.reviewStatus} /></div>
              <p>{method.description}</p>
              <small>{method.id}</small>
            </article>
          ))}
        </div>
      </Panel>

      <div className="three-column data-tools">
        <Panel title="Portable export">
          <InlineNotice tone="warning">JSON, JSONL, and Markdown are plaintext. Use encrypted backup for recovery.</InlineNotice>
          <form className="form-stack" onSubmit={exportData}>
            <Field label="New destination path"><input name="destination" required maxLength={4096} /></Field>
            <Field label="Format">
              <select name="format" defaultValue="jsonl"><option value="jsonl">JSONL</option><option value="json">JSON</option><option value="markdown">Derived Markdown</option></select>
            </Field>
            <label className="check-confirm"><input type="checkbox" name="acknowledgePlaintext" required /> I understand this artifact is plaintext.</label>
            <button className="button" disabled={busy} type="submit">Export</button>
          </form>
        </Panel>

        <Panel title="Portable import">
          <p className="secondary-copy">Import is atomic and only allowed into an empty vault. It revalidates provenance and disables provider profiles.</p>
          <form className="form-stack" onSubmit={importData}>
            <Field label="Existing JSON or JSONL path"><input name="source" required maxLength={4096} /></Field>
            <label className="check-confirm"><input type="checkbox" name="confirmEmptyVaultImport" required /> Import into this empty vault.</label>
            <button className="button" disabled={busy} type="submit">Import</button>
          </form>
        </Panel>

        <Panel title="Encrypted backup">
          <p className="secondary-copy">Creates a new authenticated <code>.qsb</code> file without plaintext temporary data.</p>
          <form className="form-stack" onSubmit={backup}>
            <Field label="New destination path"><input name="destination" required maxLength={4096} /></Field>
            <Field label="Backup recovery passphrase"><input name="backupPassphrase" type="password" required minLength={12} maxLength={1024} autoComplete="new-password" /></Field>
            <Field label="Repeat passphrase"><input name="backupConfirmation" type="password" required minLength={12} maxLength={1024} autoComplete="new-password" /></Field>
            <button className="button button-primary" disabled={busy} type="submit">Create encrypted backup</button>
          </form>
        </Panel>
      </div>
    </div>
  );
}

function clearSecretFields(form: HTMLFormElement, names: string[]) {
  for (const name of names) {
    const field = form.elements.namedItem(name);
    if (field instanceof HTMLInputElement) field.value = "";
  }
}
