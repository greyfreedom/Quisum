import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { command, failureMessage } from "../api";
import {
  EmptyState,
  Field,
  InlineNotice,
  Loading,
  PageHeader,
  Panel,
  SensitivityPill,
  SensitivitySelect,
  StatusPill,
  formatDate,
  lines,
  valueOf,
} from "../components";
import type { ClaimRecord, EventRecord, Sensitivity } from "../types";

const visibleStatuses = new Set(["confirmed", "revised", "contradicted", "uncertain"]);

export function SelfMapPage() {
  const [claims, setClaims] = useState<ClaimRecord[]>([]);
  const [events, setEvents] = useState<EventRecord[]>([]);
  const [showCandidates, setShowCandidates] = useState(false);
  const [kind, setKind] = useState("pattern_hypothesis");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [nextClaims, nextEvents] = await Promise.all([
        command<ClaimRecord[]>("claim_list", { limit: 500 }),
        command<EventRecord[]>("event_list", { limit: 500 }),
      ]);
      setClaims(nextClaims);
      setEvents(nextEvents.filter((item) => item.status === "confirmed"));
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const displayed = useMemo(
    () => claims.filter((claim) => showCandidates || visibleStatuses.has(claim.status)),
    [claims, showCandidates],
  );

  async function createClaim(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const revisit = valueOf(form, "revisitAfter");
    setBusy(true);
    try {
      await command<ClaimRecord>("claim_create", {
        kind: valueOf(form, "kind"),
        statement: valueOf(form, "statement"),
        contexts: lines(form.get("contexts")),
        supportingEvidence: form.getAll("supportingEvidence").map(String),
        counterevidence: form.getAll("counterevidence").map(String),
        alternatives: lines(form.get("alternatives")),
        evidenceGrade: valueOf(form, "evidenceGrade"),
        sensitivity: valueOf(form, "sensitivity") as Sensitivity,
        revisitAfter: revisit === "" ? null : new Date(revisit).toISOString(),
      });
      target.reset();
      setKind("pattern_hypothesis");
      setNotice("Candidate added to the human review queue. It is not yet a confirmed self claim.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function addCounterevidence(event: FormEvent<HTMLFormElement>, claimId: string) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const evidenceId = valueOf(form, "evidenceId");
    if (evidenceId === "") return;
    setBusy(true);
    try {
      await command("claim_counterevidence", {
        claimId,
        evidenceId,
        action: "propose",
      });
      await command("claim_counterevidence", {
        claimId,
        evidenceId,
        action: "accept",
      });
      setNotice("Counterevidence accepted and the claim projection was conservatively updated.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="A map that can change"
        title="Self Map"
        description="Reviewed claims keep their evidence, counterevidence, scope, and status visible. Contradiction is information, not failure."
        action={
          <label className="toggle-label">
            <input
              type="checkbox"
              checked={showCandidates}
              onChange={(event) => setShowCandidates(event.target.checked)}
            />
            Include pending and rejected
          </label>
        }
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}

      <Panel title="Propose a candidate">
        <InlineNotice tone="neutral">
          Candidates remain pending until you review them. A pattern hypothesis needs multiple supporting observations and at least one alternative explanation.
        </InlineNotice>
        <form className="form-stack" onSubmit={createClaim}>
          <div className="form-grid three">
            <Field label="Claim type">
              <select name="kind" value={kind} onChange={(event) => setKind(event.target.value)}>
                <option value="explicit_fact">Explicit fact</option>
                <option value="user_self_assessment">User self-assessment</option>
                <option value="current_state">Current state</option>
                <option value="value">Value</option>
                <option value="strength">Strength</option>
                <option value="need_or_boundary">Need or boundary</option>
                <option value="pattern_hypothesis">Pattern hypothesis</option>
              </select>
            </Field>
            <Field label="Evidence grade">
              <select name="evidenceGrade" defaultValue="limited">
                <option value="limited">Limited</option>
                <option value="moderate">Moderate</option>
                <option value="repeated">Repeated</option>
                <option value="contested">Contested</option>
              </select>
            </Field>
            <Field label="Sensitivity">
              <SensitivitySelect />
            </Field>
          </div>
          <Field label="Candidate wording" hint="Use bounded, contextual language rather than an essence or diagnosis.">
            <textarea name="statement" required rows={3} maxLength={4000} />
          </Field>
          <div className="form-grid">
            <Field label="Contexts" hint="One context per line">
              <textarea name="contexts" rows={3} maxLength={3000} />
            </Field>
            <Field label="Alternative explanations" hint="One plausible alternative per line">
              <textarea name="alternatives" rows={3} maxLength={8000} />
            </Field>
          </div>
          {kind === "current_state" && (
            <Field label="Required revisit time" hint="Current-state claims must expire into review.">
              <input type="datetime-local" name="revisitAfter" required />
            </Field>
          )}
          <div className="evidence-picker">
            <h3>Concrete event evidence</h3>
            {events.length === 0 ? (
              <EmptyState>Capture a confirmed event before linking evidence.</EmptyState>
            ) : (
              events.map((item) => (
                <div className="evidence-row" key={item.id}>
                  <div>
                    <strong>{item.title}</strong>
                    <small>{item.id}</small>
                  </div>
                  <label><input type="checkbox" name="supportingEvidence" value={item.id} /> Supports</label>
                  <label><input type="checkbox" name="counterevidence" value={item.id} /> Counters</label>
                </div>
              ))
            )}
          </div>
          <button className="button button-primary" disabled={busy} type="submit">
            {busy ? "Adding…" : "Add to review queue"}
          </button>
        </form>
      </Panel>

      <Panel title="Reviewed map" aside={<span className="count-badge">{displayed.length}</span>}>
        {loading ? (
          <Loading />
        ) : displayed.length === 0 ? (
          <EmptyState>No reviewed claims yet. The map is allowed to begin empty.</EmptyState>
        ) : (
          <div className="claim-grid">
            {displayed.map((claim) => (
              <article className="claim-card" key={claim.id}>
                <div className="item-meta">
                  <StatusPill value={claim.status} />
                  <StatusPill value={claim.kind} />
                  <SensitivityPill value={claim.sensitivity} />
                </div>
                <blockquote>{claim.statement}</blockquote>
                <dl className="compact-facts">
                  <div><dt>Evidence</dt><dd>{claim.supporting_evidence.length} supporting</dd></div>
                  <div><dt>Counterevidence</dt><dd>{claim.counterevidence.length} limiting</dd></div>
                  <div><dt>Grade</dt><dd>{claim.evidence_grade}</dd></div>
                  <div><dt>Updated</dt><dd>{formatDate(claim.updated_at)}</dd></div>
                </dl>
                {claim.contexts.length > 0 && <p className="secondary-copy"><strong>Contexts:</strong> {claim.contexts.join(" · ")}</p>}
                {claim.alternative_explanations.length > 0 && (
                  <details>
                    <summary>Alternative explanations</summary>
                    <ul>{claim.alternative_explanations.map((item) => <li key={item}>{item}</li>)}</ul>
                  </details>
                )}
                {visibleStatuses.has(claim.status) && events.length > 0 && (
                  <form className="inline-form counter-form" onSubmit={(event) => void addCounterevidence(event, claim.id)}>
                    <select name="evidenceId" defaultValue="" aria-label={`Counterevidence for ${claim.statement}`}>
                      <option value="">Add counterevidence…</option>
                      {events
                        .filter((item) => !claim.supporting_evidence.includes(item.id) && !claim.counterevidence.includes(item.id))
                        .map((item) => <option key={item.id} value={item.id}>{item.title}</option>)}
                    </select>
                    <button className="button" disabled={busy} type="submit">Review & add</button>
                  </form>
                )}
                <small className="record-id">{claim.id}</small>
              </article>
            ))}
          </div>
        )}
      </Panel>
    </div>
  );
}
