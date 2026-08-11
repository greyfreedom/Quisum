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
  SensitivitySelect,
  StatusPill,
  formatDate,
  lines,
  valueOf,
} from "../components";
import type { EventRecord, SearchResult, Sensitivity } from "../types";

export function TimelinePage() {
  const [events, setEvents] = useState<EventRecord[]>([]);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [deleteTarget, setDeleteTarget] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setEvents(await command<EventRecord[]>("event_list", { limit: 300 }));
      setNotice(undefined);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function capture(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    const occurred = valueOf(form, "occurredAt");
    setBusy(true);
    try {
      await command<EventRecord>("event_capture", {
        title: valueOf(form, "title"),
        facts: lines(form.get("facts")),
        interpretations: lines(form.get("interpretations")),
        emotions: lines(form.get("emotions")),
        bodilyResponses: lines(form.get("bodilyResponses")),
        actions: lines(form.get("actions")),
        shortTermOutcomes: lines(form.get("shortTermOutcomes")),
        longTermOutcomes: lines(form.get("longTermOutcomes")),
        topics: lines(form.get("topics")),
        sensitivity: valueOf(form, "sensitivity") as Sensitivity,
        occurredAt: occurred === "" ? null : new Date(occurred).toISOString(),
      });
      target.reset();
      setNotice("Event captured as a concrete, user-confirmed record.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function review(id: string, accept: boolean) {
    setBusy(true);
    try {
      await command<EventRecord>("event_review", { id, accept });
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    setBusy(true);
    try {
      await command("event_delete", { id });
      setDeleteTarget(undefined);
      setNotice("Event deleted; dependent claims were repaired or purged atomically.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    try {
      setResults(
        await command<SearchResult[]>("memory_search", {
          query: valueOf(form, "query"),
          limit: 20,
        }),
      );
    } catch (error) {
      setNotice(failureMessage(error));
    }
  }

  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Observe before interpreting"
        title="Timeline"
        description="Keep concrete facts, interpretations, actions, and outcomes distinct. Direct captures are yours—not model verdicts."
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}

      <div className="two-column wide-left">
        <Panel title="Capture an event" className="sticky-panel">
          <form className="form-stack" onSubmit={capture}>
            <Field label="Short title">
              <input name="title" required maxLength={240} autoComplete="off" />
            </Field>
            <Field label="Observable or directly reported facts" hint="One fact per line. At least one is required.">
              <textarea name="facts" required rows={4} maxLength={12000} />
            </Field>
            <Field label="Your interpretations" hint="Keep these separate from what happened.">
              <textarea name="interpretations" rows={3} maxLength={8000} />
            </Field>
            <div className="form-grid">
              <Field label="Emotions" hint="One per line">
                <textarea name="emotions" rows={3} maxLength={3000} />
              </Field>
              <Field label="Bodily responses" hint="One per line">
                <textarea name="bodilyResponses" rows={3} maxLength={6000} />
              </Field>
              <Field label="Actions" hint="One per line">
                <textarea name="actions" rows={3} maxLength={6000} />
              </Field>
              <Field label="Short-term outcomes" hint="One per line">
                <textarea name="shortTermOutcomes" rows={3} maxLength={6000} />
              </Field>
            </div>
            <details>
              <summary>More context</summary>
              <div className="form-grid detail-grid">
                <Field label="Long-term outcomes" hint="One per line">
                  <textarea name="longTermOutcomes" rows={3} maxLength={6000} />
                </Field>
                <Field label="Topics" hint="One short topic per line">
                  <textarea name="topics" rows={3} maxLength={3000} />
                </Field>
                <Field label="When it happened">
                  <input name="occurredAt" type="datetime-local" />
                </Field>
                <Field label="Sensitivity">
                  <SensitivitySelect />
                </Field>
              </div>
            </details>
            <button className="button button-primary" disabled={busy} type="submit">
              {busy ? "Saving…" : "Capture event"}
            </button>
          </form>
        </Panel>

        <div className="page-stack compact">
          <Panel title="Search local memory">
            <form className="inline-form" onSubmit={search}>
              <input name="query" required maxLength={4000} placeholder="Search without sending anything" />
              <button className="button" type="submit">Search</button>
            </form>
            {results.length > 0 && (
              <ul className="search-results">
                {results.map((result) => (
                  <li key={result.itemId}>
                    <div className="item-meta">
                      <StatusPill value={result.itemType} />
                      <SensitivityPill value={result.sensitivity} />
                    </div>
                    <p>{result.excerpt}</p>
                    <small>{result.selectionReason}</small>
                  </li>
                ))}
              </ul>
            )}
          </Panel>

          <Panel title="Recorded events" aside={<span className="count-badge">{events.length}</span>}>
            {loading ? (
              <Loading />
            ) : events.length === 0 ? (
              <EmptyState>Your event timeline is empty.</EmptyState>
            ) : (
              <div className="card-list">
                {events.map((item) => (
                  <article className="record-card" key={item.id}>
                    <div className="record-card-header">
                      <div>
                        <h3>{item.title}</h3>
                        <p className="timestamp">{formatDate(item.occurred_at ?? item.recorded_at)}</p>
                      </div>
                      <div className="item-meta">
                        <StatusPill value={item.status} />
                        <SensitivityPill value={item.sensitivity} />
                      </div>
                    </div>
                    <ul className="fact-list">
                      {item.facts.map((fact) => <li key={fact}>{fact}</li>)}
                    </ul>
                    {item.interpretations.length > 0 && (
                      <p className="secondary-copy"><strong>Interpretation:</strong> {item.interpretations.join(" · ")}</p>
                    )}
                    {item.status === "proposed" && (
                      <div className="button-row">
                        <button className="button button-primary" disabled={busy} onClick={() => void review(item.id, true)}>Accept</button>
                        <button className="button" disabled={busy} onClick={() => void review(item.id, false)}>Reject</button>
                      </div>
                    )}
                    {deleteTarget === item.id ? (
                      <div className="danger-confirm">
                        <p>Delete this event and atomically repair every dependent claim?</p>
                        <div className="button-row">
                          <button className="button button-danger" disabled={busy} onClick={() => void remove(item.id)}>Confirm delete</button>
                          <button className="button" onClick={() => setDeleteTarget(undefined)}>Cancel</button>
                        </div>
                      </div>
                    ) : (
                      <button className="text-button danger-text" onClick={() => setDeleteTarget(item.id)}>Delete event</button>
                    )}
                  </article>
                ))}
              </div>
            )}
          </Panel>
        </div>
      </div>
    </div>
  );
}
