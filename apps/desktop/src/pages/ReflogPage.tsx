import { useCallback, useEffect, useState } from "react";
import { command, failureMessage } from "../api";
import {
  EmptyState,
  InlineNotice,
  Loading,
  PageHeader,
  Panel,
  StatusPill,
  formatDate,
  humanize,
} from "../components";
import type { ReflogEntry } from "../types";

interface DoctorResult {
  ok: boolean;
  sqlcipher: string;
  integrity: string;
  schemaVersion: number;
  latestSupportedSchema: number;
  recoveryEnvelopePresent: boolean;
  telemetry: boolean;
}

export function ReflogPage() {
  const [entries, setEntries] = useState<ReflogEntry[]>([]);
  const [doctor, setDoctor] = useState<DoctorResult>();
  const [loading, setLoading] = useState(true);
  const [checking, setChecking] = useState(false);
  const [notice, setNotice] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setEntries(await command<ReflogEntry[]>("reflog_list", { limit: 1000 }));
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function runDoctor() {
    setChecking(true);
    try {
      setDoctor(await command<DoctorResult>("vault_doctor"));
      setNotice("SQLCipher, schema, and database integrity checks passed.");
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Structural history"
        title="Reflog"
        description="An append-only, content-free account of what changed, when, and by which authority. Private record text is intentionally absent."
        action={<button className="button" disabled={checking} onClick={() => void runDoctor()}>{checking ? "Checking…" : "Run vault doctor"}</button>}
      />
      {notice !== undefined && <InlineNotice tone={doctor?.ok === true ? "success" : "neutral"}>{notice}</InlineNotice>}
      {doctor !== undefined && (
        <Panel title="Integrity result">
          <div className="metric-grid">
            <div><span>SQLCipher</span><strong>{doctor.sqlcipher}</strong></div>
            <div><span>Database</span><strong>{doctor.integrity}</strong></div>
            <div><span>Schema</span><strong>{doctor.schemaVersion} / {doctor.latestSupportedSchema}</strong></div>
            <div><span>Recovery</span><strong>{doctor.recoveryEnvelopePresent ? "present" : "missing"}</strong></div>
            <div><span>Telemetry</span><strong>{doctor.telemetry ? "enabled" : "none"}</strong></div>
          </div>
        </Panel>
      )}
      <Panel title="Local audit ledger" aside={<span className="count-badge">{entries.length}</span>}>
        {loading ? (
          <Loading />
        ) : entries.length === 0 ? (
          <EmptyState>No audit entries were returned.</EmptyState>
        ) : (
          <ol className="reflog-list">
            {entries.map((entry) => (
              <li key={entry.sequence}>
                <span className="sequence">#{entry.sequence}</span>
                <div>
                  <strong>{humanize(entry.event.action)}</strong>
                  <p>
                    {entry.event.entity_type !== undefined ? humanize(entry.event.entity_type) : "Vault operation"}
                    {entry.event.entity_id !== undefined ? ` · ${entry.event.entity_id}` : ""}
                  </p>
                </div>
                <StatusPill value={entry.event.actor} />
                <time>{formatDate(entry.event.created_at)}</time>
              </li>
            ))}
          </ol>
        )}
      </Panel>
    </div>
  );
}
