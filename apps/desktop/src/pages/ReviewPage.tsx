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
import type { ClaimRecord, ContextPacket, EventRecord, OpenQuestion } from "../types";

export function ReviewPage() {
  const [claims, setClaims] = useState<ClaimRecord[]>([]);
  const [events, setEvents] = useState<EventRecord[]>([]);
  const [questions, setQuestions] = useState<OpenQuestion[]>([]);
  const [contexts, setContexts] = useState<Record<string, ContextPacket>>({});
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [nextClaims, nextEvents, nextQuestions] = await Promise.all([
        command<ClaimRecord[]>("claim_review_queue", { limit: 100 }),
        command<EventRecord[]>("event_list", { limit: 500 }),
        command<OpenQuestion[]>("question_list", { limit: 500 }),
      ]);
      const contextIds = [...new Set(nextClaims.flatMap((claim) => claim.source.context_packet_id === undefined ? [] : [claim.source.context_packet_id]))];
      const contextEntries = await Promise.all(contextIds.map(async (id) => [id, await command<ContextPacket>("context_detail", { id })] as const));
      setClaims(nextClaims);
      setEvents(nextEvents);
      setQuestions(nextQuestions.filter((item) => item.status === "proposed" || item.status === "open"));
      setContexts(Object.fromEntries(contextEntries));
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function reviewClaim(id: string, event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    try {
      await command("claim_review", {
        id,
        decision: valueOf(form, "decision"),
        editedStatement: valueOf(form, "editedStatement") || null,
        note: valueOf(form, "note") || null,
      });
      setNotice("Review committed with an append-only revision.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function reviewEvent(id: string, accept: boolean) {
    setBusy(true);
    try {
      await command("event_review", { id, accept });
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function reviewQuestion(id: string, target: "open" | "answered" | "closed") {
    setBusy(true);
    try {
      await command("question_review", { id, target });
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  const proposedEvents = events.filter((item) => item.status === "proposed");
  const total = claims.length + proposedEvents.length + questions.length;

  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Human authority"
        title="Review"
        description="Models and imports may propose. Only you can accept, edit, keep uncertain, reject, or turn a claim back into an open question."
        action={<span className="review-total">{total} item{total === 1 ? "" : "s"}</span>}
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}
      {loading ? (
        <Loading />
      ) : total === 0 ? (
        <Panel><EmptyState>The review queue is clear.</EmptyState></Panel>
      ) : (
        <>
          <Panel title="Claim candidates" aside={<span className="count-badge">{claims.length}</span>}>
            {claims.length === 0 ? (
              <EmptyState>No claim needs a decision.</EmptyState>
            ) : (
              <div className="review-list">
                {claims.map((claim) => (
                  <ReviewClaimCard
                    busy={busy}
                    claim={claim}
                    evidenceEvents={events}
                    sourceContext={claim.source.context_packet_id === undefined ? undefined : contexts[claim.source.context_packet_id]}
                    key={claim.id}
                    onReview={(event) => void reviewClaim(claim.id, event)}
                  />
                ))}
              </div>
            )}
          </Panel>

          <div className="two-column">
            <Panel title="Proposed events" aside={<span className="count-badge">{proposedEvents.length}</span>}>
              {proposedEvents.length === 0 ? (
                <EmptyState>No proposed event needs review.</EmptyState>
              ) : proposedEvents.map((item) => (
                <article className="review-card" key={item.id}>
                  <div className="record-card-header">
                    <div><h3>{item.title}</h3><small>{formatDate(item.recorded_at)}</small></div>
                    <SensitivityPill value={item.sensitivity} />
                  </div>
                  <ul className="fact-list">{item.facts.map((fact) => <li key={fact}>{fact}</li>)}</ul>
                  <p className="source-line">Proposed by {item.source.provider ?? item.source.actor}</p>
                  <div className="button-row">
                    <button className="button button-primary" disabled={busy} onClick={() => void reviewEvent(item.id, true)}>Accept event</button>
                    <button className="button" disabled={busy} onClick={() => void reviewEvent(item.id, false)}>Reject</button>
                  </div>
                </article>
              ))}
            </Panel>

            <Panel title="Open questions" aside={<span className="count-badge">{questions.length}</span>}>
              {questions.length === 0 ? (
                <EmptyState>No question needs a lifecycle decision.</EmptyState>
              ) : questions.map((item) => (
                <article className="review-card" key={item.id}>
                  <div className="item-meta">
                    <StatusPill value={item.status} />
                    <SensitivityPill value={item.sensitivity} />
                  </div>
                  <h3>{item.question}</h3>
                  <p className="source-line">Source: {sourceDescription(item.source)}</p>
                  <div className="button-row">
                    {item.status === "proposed" ? (
                      <>
                        <button className="button button-primary" disabled={busy} onClick={() => void reviewQuestion(item.id, "open")}>Keep open</button>
                        <button className="button" disabled={busy} onClick={() => void reviewQuestion(item.id, "closed")}>Reject</button>
                      </>
                    ) : (
                      <>
                        <button className="button button-primary" disabled={busy} onClick={() => void reviewQuestion(item.id, "answered")}>Mark answered</button>
                        <button className="button" disabled={busy} onClick={() => void reviewQuestion(item.id, "closed")}>Close without answer</button>
                      </>
                    )}
                  </div>
                </article>
              ))}
            </Panel>
          </div>
        </>
      )}
    </div>
  );
}

function ReviewClaimCard({
  claim,
  evidenceEvents,
  sourceContext,
  busy,
  onReview,
}: {
  claim: ClaimRecord;
  evidenceEvents: EventRecord[];
  sourceContext?: ContextPacket;
  busy: boolean;
  onReview: (event: FormEvent<HTMLFormElement>) => void;
}) {
  const [decision, setDecision] = useState("confirm");
  const asksForWording = decision === "revise_and_confirm" || decision === "convert_to_open_question";
  return (
    <article className="review-card review-claim-card">
      <div className="record-card-header">
        <div className="item-meta">
          <StatusPill value={claim.status} />
          <StatusPill value={claim.kind} />
          <SensitivityPill value={claim.sensitivity} />
        </div>
        <small>{formatDate(claim.updated_at)}</small>
      </div>
      <blockquote>{claim.statement}</blockquote>
      <div className="evidence-summary">
        <span>{claim.supporting_evidence.length} supporting source(s)</span>
        <span>{claim.counterevidence.length} counterexample(s)</span>
        <span>Evidence: {claim.evidence_grade}</span>
      </div>
      <div className="review-evidence">
        <h4>Evidence to inspect</h4>
        <EvidenceLinks label="Supporting" ids={claim.supporting_evidence} events={evidenceEvents} />
        <EvidenceLinks label="Counterevidence" ids={claim.counterevidence} events={evidenceEvents} />
      </div>
      {claim.contexts.length > 0 && <p className="secondary-copy"><strong>Contexts:</strong> {claim.contexts.join(" · ")}</p>}
      {claim.alternative_explanations.length > 0 && (
        <details>
          <summary>Inspect alternative explanations</summary>
          <ul>{claim.alternative_explanations.map((item) => <li key={item}>{item}</li>)}</ul>
        </details>
      )}
      <p className="source-line">Source: {sourceDescription(claim.source)}</p>
      {sourceContext !== undefined && (
        <details className="source-context">
          <summary>Inspect the exact model context</summary>
          <p><strong>Request:</strong> {sourceContext.query}</p>
          {sourceContext.items.length === 0 ? (
            <p>No prior-memory excerpt was sent.</p>
          ) : (
            <ul>
              {sourceContext.items.map((item) => (
                <li key={item.item_id}>
                  <div className="item-meta">
                    <StatusPill value={item.item_type} />
                    <SensitivityPill value={item.sensitivity} />
                    {(claim.supporting_evidence.includes(item.item_id) || claim.counterevidence.includes(item.item_id)) && <StatusPill value="linked_evidence" />}
                  </div>
                  <blockquote>{item.excerpt}</blockquote>
                  <code>{item.item_id}</code>
                </li>
              ))}
            </ul>
          )}
        </details>
      )}
      <form className="review-form" onSubmit={onReview}>
        <Field label="Decision">
          <select name="decision" value={decision} onChange={(event) => setDecision(event.target.value)}>
            <option value="confirm">Confirm as worded</option>
            <option value="revise_and_confirm">Revise and confirm</option>
            <option value="uncertain">Keep uncertain</option>
            <option value="reject">Reject</option>
            <option value="convert_to_open_question">Convert to open question</option>
          </select>
        </Field>
        {asksForWording && (
          <Field label={decision === "revise_and_confirm" ? "Revised wording" : "Question wording"}>
            <textarea
              name="editedStatement"
              required={decision === "revise_and_confirm"}
              defaultValue={claim.statement}
              rows={2}
              maxLength={4000}
            />
          </Field>
        )}
        <Field label="Private review note">
          <textarea name="note" rows={2} maxLength={4000} />
        </Field>
        <button className="button button-primary" disabled={busy} type="submit">Commit decision</button>
      </form>
    </article>
  );
}

function EvidenceLinks({ label, ids, events }: { label: string; ids: string[]; events: EventRecord[] }) {
  const byId = new Map(events.map((event) => [event.id, event]));
  return (
    <div>
      <strong>{label}</strong>
      {ids.length === 0 ? (
        <span>None linked</span>
      ) : (
        <ul>
          {ids.map((id) => {
            const event = byId.get(id);
            return (
              <li key={id}>
                <code>{id}</code>
                {event !== undefined && <><b>{event.title}</b><span>{event.facts.slice(0, 2).join(" · ")}</span></>}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function sourceDescription(source: ClaimRecord["source"]): string {
  const parts = [source.provider ?? source.actor, source.model, source.method];
  if (source.context_packet_id !== undefined) parts.push(`context ${source.context_packet_id}`);
  if (source.import_id !== undefined) parts.push(`import ${source.import_id}`);
  return parts.filter((part): part is string => part !== undefined).join(" · ");
}
