import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { command, failureGuidance, failureMessage, type CrisisGuidance } from "../api";
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
  valueOf,
} from "../components";
import type {
  ContextPacket,
  MethodSummary,
  ProviderProfile,
  ReflectionSession,
  Sensitivity,
  SessionDetail,
} from "../types";

interface ExploreResponse {
  sessionId: string;
  userMessageId: string;
  assistantMessageId: string;
  response: {
    text: string;
    model?: string;
    finish_reason?: string;
    usage: { input_tokens?: number; output_tokens?: number };
  };
}

export function ExplorePage() {
  const [methods, setMethods] = useState<MethodSummary[]>([]);
  const [providers, setProviders] = useState<ProviderProfile[]>([]);
  const [sessions, setSessions] = useState<ReflectionSession[]>([]);
  const [packet, setPacket] = useState<ContextPacket>();
  const [answer, setAnswer] = useState<ExploreResponse>();
  const [detail, setDetail] = useState<SessionDetail>();
  const [deleteTarget, setDeleteTarget] = useState<string>();
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [crisisGuidance, setCrisisGuidance] = useState<CrisisGuidance>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [nextMethods, nextProviders, nextSessions] = await Promise.all([
        command<MethodSummary[]>("method_list"),
        command<ProviderProfile[]>("provider_list"),
        command<ReflectionSession[]>("session_list", { limit: 200 }),
      ]);
      setMethods(nextMethods);
      setProviders(nextProviders);
      setSessions(nextSessions);
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const enabledProviders = useMemo(() => providers.filter((profile) => profile.enabled), [providers]);
  const activeSessions = useMemo(() => sessions.filter((session) => session.status === "active"), [sessions]);

  async function preview(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setAnswer(undefined);
    setCrisisGuidance(undefined);
    try {
      const nextPacket = await command<ContextPacket>("context_preview", {
        providerId: valueOf(form, "providerId"),
        purpose: valueOf(form, "purpose"),
        method: valueOf(form, "method"),
        query: valueOf(form, "query"),
        querySensitivity: valueOf(form, "querySensitivity") as Sensitivity,
        maximumItems: Number(valueOf(form, "maximumItems")),
        region: navigator.language || null,
      });
      setPacket(nextPacket);
      setNotice("Context preview built locally. Nothing has been sent to the provider.");
    } catch (error) {
      const guidance = failureGuidance(error);
      setCrisisGuidance(guidance);
      setNotice(guidance === undefined
        ? failureMessage(error)
        : "Quisum stopped identity analysis and did not contact the provider.");
    } finally {
      setBusy(false);
    }
  }

  async function approve() {
    if (packet === undefined) return;
    setBusy(true);
    try {
      setPacket(await command<ContextPacket>("context_approve", { id: packet.id }));
      setNotice("One-time approval recorded for this exact context packet.");
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function send(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (packet === undefined) return;
    const form = new FormData(event.currentTarget);
    setBusy(true);
    try {
      const response = await command<ExploreResponse>("explore_send", {
        contextId: packet.id,
        sessionId: valueOf(form, "sessionId") || null,
        title: valueOf(form, "title") || null,
        maxOutputTokens: Number(valueOf(form, "maxOutputTokens")),
      });
      setAnswer(response);
      setPacket(undefined);
      setNotice("The provider response passed local protocol and safety validation and was saved to the encrypted transcript.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function loadSession(id: string) {
    try {
      setDetail(await command<SessionDetail>("session_detail", { id, messageLimit: 2000 }));
    } catch (error) {
      setNotice(failureMessage(error));
    }
  }

  async function transitionSession(id: string, action: "complete" | "abandon" | "delete") {
    setBusy(true);
    try {
      await command("session_transition", {
        id,
        action,
        confirmDelete: action === "delete",
      });
      setDeleteTarget(undefined);
      setDetail(undefined);
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
        eyebrow="Purpose before disclosure"
        title="Explore"
        description="Choose a method and provider, inspect the minimum context, then decide whether to send. Memory is quoted data—not instructions."
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}
      {crisisGuidance !== undefined && (
        <Panel title="Immediate support" className="crisis-guidance">
          <p>{crisisGuidance.assistantResponse}</p>
          <p className="secondary-copy">{crisisGuidance.limitations}</p>
          <div className="item-meta">
            <span>Resource pack {crisisGuidance.packVersion}</span>
            <span>Region {crisisGuidance.region}</span>
            <span>Sources checked {crisisGuidance.sourceCheckedAt}</span>
            <span>{crisisGuidance.isCurrent ? "Current source check" : "Source check overdue"} · due {crisisGuidance.sourceCheckDue}</span>
          </div>
          {crisisGuidance.sourceUrls.length > 0 && (
            <details>
              <summary>Authoritative source addresses</summary>
              <ul className="compact-list">
                {crisisGuidance.sourceUrls.map((source) => <li key={source}><code>{source}</code></li>)}
              </ul>
            </details>
          )}
        </Panel>
      )}
      {loading ? (
        <Loading />
      ) : enabledProviders.length === 0 ? (
        <Panel>
          <EmptyState>Add and enable a provider in Privacy & Models before starting an independent reflection.</EmptyState>
        </Panel>
      ) : (
        <div className="two-column wide-left">
          <div className="page-stack compact">
            <Panel title="1 · Frame the request">
              <form className="form-stack" onSubmit={preview}>
                <div className="form-grid">
                  <Field label="Method">
                    <select name="method" defaultValue={methods[0]?.id}>
                      {methods.map((method) => (
                        <option key={method.id} value={method.id}>
                          {method.title} · {method.reviewStatus}
                        </option>
                      ))}
                    </select>
                  </Field>
                  <Field label="Provider">
                    <select name="providerId" defaultValue={enabledProviders[0]?.id}>
                      {enabledProviders.map((profile) => (
                        <option key={profile.id} value={profile.id}>
                          {profile.name} · {profile.privacy_kind}
                        </option>
                      ))}
                    </select>
                  </Field>
                </div>
                <Field label="Declared purpose">
                  <input name="purpose" required maxLength={240} defaultValue="Reflect on the current question without making identity verdicts" />
                </Field>
                <Field label="What would you like to explore?">
                  <textarea name="query" required rows={7} maxLength={4000} />
                </Field>
                <div className="form-grid">
                  <Field label="Query sensitivity">
                    <SensitivitySelect name="querySensitivity" providerEligibleOnly />
                  </Field>
                  <Field label="Maximum memory items" hint="Smaller is usually better.">
                    <input name="maximumItems" type="number" min={1} max={64} defaultValue={8} required />
                  </Field>
                </div>
                <button className="button button-primary" disabled={busy} type="submit">
                  {busy ? "Building locally…" : "Build context preview"}
                </button>
              </form>
            </Panel>

            {packet !== undefined && (
              <Panel title="2 · Inspect exact disclosure" className="context-panel">
                <div className="context-summary">
                  <div><span>Provider model</span><strong>{packet.model}</strong></div>
                  <div><span>Query sensitivity</span><SensitivityPill value={packet.query_sensitivity} /></div>
                  <div><span>Included</span><strong>{packet.items.length}</strong></div>
                  <div><span>Excluded</span><strong>{packet.exclusions.length}</strong></div>
                </div>
                <div className="query-preview"><span>Your query</span><p>{packet.query}</p></div>
                {packet.items.length === 0 ? (
                  <EmptyState>No prior memory was selected; only the current query would be sent.</EmptyState>
                ) : (
                  <div className="context-items">
                    {packet.items.map((item) => (
                      <article key={item.item_id}>
                        <div className="item-meta">
                          <StatusPill value={item.item_type} />
                          <SensitivityPill value={item.sensitivity} />
                        </div>
                        <blockquote>{item.excerpt}</blockquote>
                        <small>{item.selection_reason}</small>
                      </article>
                    ))}
                  </div>
                )}
                {packet.exclusions.length > 0 && (
                  <details>
                    <summary>{packet.exclusions.length} local record(s) excluded by relevance or policy</summary>
                    <ul className="compact-list">
                      {packet.exclusions.map((item) => <li key={item.item_id}>{item.item_id} · {item.reason}</li>)}
                    </ul>
                  </details>
                )}
                {packet.requires_approval && packet.approved_at === undefined && (
                  <InlineNotice tone="warning">
                    This exact packet contains highly sensitive material. Review every excerpt, then record one-time approval.
                    <button className="button button-warning" disabled={busy} onClick={() => void approve()}>Approve this packet only</button>
                  </InlineNotice>
                )}
                {(!packet.requires_approval || packet.approved_at !== undefined) && (
                  <form className="form-stack send-form" onSubmit={send}>
                    <div className="form-grid">
                      <Field label="Continue active session">
                        <select name="sessionId" defaultValue="">
                          <option value="">Start a new session</option>
                          {activeSessions
                            .filter((session) => session.method === packet.method)
                            .map((session) => (
                              <option key={session.id} value={session.id}>
                                {session.title ?? "Untitled"} · {session.sensitivity}
                              </option>
                            ))}
                        </select>
                      </Field>
                      <Field label="New session title" hint="Used only when starting a new session.">
                        <input name="title" maxLength={240} />
                      </Field>
                    </div>
                    <Field label="Maximum response tokens">
                      <input name="maxOutputTokens" type="number" min={1} max={32768} defaultValue={2048} required />
                    </Field>
                    <button className="button button-primary" disabled={busy} type="submit">
                      {busy ? "Validating and sending…" : "Send exactly this packet"}
                    </button>
                  </form>
                )}
              </Panel>
            )}

            {answer !== undefined && (
              <Panel title="Validated response">
                <div className="assistant-answer">{answer.response.text}</div>
                <div className="item-meta response-meta">
                  <span>Session {answer.sessionId}</span>
                  <span>{answer.response.model ?? "Configured model"}</span>
                  <span>{answer.response.usage.output_tokens ?? "—"} output tokens</span>
                </div>
              </Panel>
            )}
          </div>

          <Panel title="Sessions" aside={<span className="count-badge">{sessions.length}</span>} className="sticky-panel session-panel">
            {sessions.length === 0 ? (
              <EmptyState>No reflection sessions yet.</EmptyState>
            ) : (
              <div className="session-list">
                {sessions.map((session) => (
                  <button className="session-row" key={session.id} onClick={() => void loadSession(session.id)}>
                    <span><strong>{session.title ?? "Untitled reflection"}</strong><small>{formatDate(session.started_at)}</small></span>
                    <StatusPill value={session.status} />
                  </button>
                ))}
              </div>
            )}
            {detail !== undefined && (
              <div className="session-detail">
                <div className="record-card-header">
                  <div><h3>{detail.session.title ?? "Untitled reflection"}</h3><small>{detail.session.method}</small></div>
                  <SensitivityPill value={detail.session.sensitivity} />
                </div>
                <div className="transcript">
                  {detail.messages.map((message) => (
                    <article className={`message message-${message.role}`} key={message.id}>
                      <strong>{message.role === "user" ? "You" : "Model response"}</strong>
                      <p>{message.content}</p>
                    </article>
                  ))}
                </div>
                {detail.session.status === "active" && (
                  <div className="button-row">
                    <button className="button button-primary" disabled={busy} onClick={() => void transitionSession(detail.session.id, "complete")}>Complete</button>
                    <button className="button" disabled={busy} onClick={() => void transitionSession(detail.session.id, "abandon")}>Abandon without conclusion</button>
                  </div>
                )}
                {deleteTarget === detail.session.id ? (
                  <div className="danger-confirm">
                    <p>Delete the encrypted transcript and repair dependent claims?</p>
                    <div className="button-row">
                      <button className="button button-danger" disabled={busy} onClick={() => void transitionSession(detail.session.id, "delete")}>Confirm delete</button>
                      <button className="button" onClick={() => setDeleteTarget(undefined)}>Cancel</button>
                    </div>
                  </div>
                ) : (
                  <button className="text-button danger-text" onClick={() => setDeleteTarget(detail.session.id)}>Delete transcript</button>
                )}
              </div>
            )}
          </Panel>
        </div>
      )}
    </div>
  );
}
