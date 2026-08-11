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
import type { Experiment, Sensitivity } from "../types";

export function ExperimentsPage() {
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [completionTarget, setCompletionTarget] = useState<string>();

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setExperiments(await command<Experiment[]>("experiment_list", { limit: 300 }));
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = event.currentTarget;
    const form = new FormData(target);
    setBusy(true);
    try {
      await command("experiment_create", {
        title: valueOf(form, "title"),
        hypothesis: valueOf(form, "hypothesis"),
        prediction: valueOf(form, "prediction"),
        action: valueOf(form, "action"),
        stopCondition: valueOf(form, "stopCondition"),
        risk: valueOf(form, "risk"),
        relatedClaims: lines(form.get("relatedClaims")),
        expectedResult: valueOf(form, "expectedResult") || null,
        sensitivity: valueOf(form, "sensitivity") as Sensitivity,
      });
      target.reset();
      setNotice("Experiment proposed. Activation remains a separate human decision.");
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function transition(
    id: string,
    action: "activate" | "cancel",
  ) {
    setBusy(true);
    try {
      await command("experiment_transition", {
        id,
        action,
        actualResult: null,
        learning: null,
      });
      await reload();
    } catch (error) {
      setNotice(failureMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function complete(id: string, event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    try {
      await command("experiment_transition", {
        id,
        action: "complete",
        actualResult: valueOf(form, "actualResult"),
        learning: valueOf(form, "learning"),
      });
      setCompletionTarget(undefined);
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
        eyebrow="Small tests, not self-verdicts"
        title="Experiments"
        description="Turn a hypothesis into one voluntary, bounded, reversible action with a prediction recorded in advance and a clear stop condition."
      />
      {notice !== undefined && <InlineNotice>{notice}</InlineNotice>}

      <div className="two-column wide-left">
        <Panel title="Propose an experiment" className="sticky-panel">
          <form className="form-stack" onSubmit={create}>
            <Field label="Title"><input name="title" required maxLength={240} /></Field>
            <Field label="Hypothesis" hint="What contextual explanation are you testing?">
              <textarea name="hypothesis" required rows={3} maxLength={4000} />
            </Field>
            <Field label="Observable prediction" hint="Write this before acting.">
              <textarea name="prediction" required rows={3} maxLength={4000} />
            </Field>
            <Field label="One bounded action">
              <textarea name="action" required rows={3} maxLength={4000} />
            </Field>
            <Field label="Stop condition">
              <textarea name="stopCondition" required rows={2} maxLength={1000} />
            </Field>
            <div className="form-grid">
              <Field label="Risk">
                <select name="risk" defaultValue="low">
                  <option value="low">Low</option>
                  <option value="moderate">Moderate</option>
                </select>
              </Field>
              <Field label="Sensitivity"><SensitivitySelect /></Field>
            </div>
            <Field label="Expected result" hint="Optional; recording it reduces hindsight bias.">
              <textarea name="expectedResult" rows={2} maxLength={4000} />
            </Field>
            <Field label="Related reviewed claim IDs" hint="Optional, one ID per line">
              <textarea name="relatedClaims" rows={2} maxLength={3000} />
            </Field>
            <button className="button button-primary" disabled={busy} type="submit">
              {busy ? "Saving…" : "Propose experiment"}
            </button>
          </form>
        </Panel>

        <Panel title="Experiment log" aside={<span className="count-badge">{experiments.length}</span>}>
          {loading ? (
            <Loading />
          ) : experiments.length === 0 ? (
            <EmptyState>No experiments yet. There is no pressure to create one.</EmptyState>
          ) : (
            <div className="card-list">
              {experiments.map((item) => (
                <article className="experiment-card" key={item.id}>
                  <div className="record-card-header">
                    <div><h3>{item.title}</h3><small>{formatDate(item.planned_at)}</small></div>
                    <div className="item-meta">
                      <StatusPill value={item.status} />
                      <StatusPill value={`${item.risk_level}_risk`} />
                      <SensitivityPill value={item.sensitivity} />
                    </div>
                  </div>
                  <dl className="experiment-steps">
                    <div><dt>Hypothesis</dt><dd>{item.hypothesis}</dd></div>
                    <div><dt>Prediction</dt><dd>{item.prediction}</dd></div>
                    <div><dt>Action</dt><dd>{item.action}</dd></div>
                    <div><dt>Stop if</dt><dd>{item.stop_condition}</dd></div>
                  </dl>
                  {item.expected_result !== undefined && <p className="secondary-copy"><strong>Expected:</strong> {item.expected_result}</p>}
                  {item.status === "proposed" && (
                    <div className="button-row">
                      <button className="button button-primary" disabled={busy} onClick={() => void transition(item.id, "activate")}>Activate voluntarily</button>
                      <button className="button" disabled={busy} onClick={() => void transition(item.id, "cancel")}>Cancel</button>
                    </div>
                  )}
                  {item.status === "active" && completionTarget !== item.id && (
                    <div className="button-row">
                      <button className="button button-primary" onClick={() => setCompletionTarget(item.id)}>Record result</button>
                      <button className="button" disabled={busy} onClick={() => void transition(item.id, "cancel")}>Stop without penalty</button>
                    </div>
                  )}
                  {item.status === "active" && completionTarget === item.id && (
                    <form className="form-stack inset-form" onSubmit={(event) => void complete(item.id, event)}>
                      <Field label="What actually happened?"><textarea name="actualResult" required rows={3} maxLength={4000} /></Field>
                      <Field label="What did you learn or revise?"><textarea name="learning" required rows={3} maxLength={4000} /></Field>
                      <div className="button-row">
                        <button className="button button-primary" disabled={busy} type="submit">Complete</button>
                        <button className="button" type="button" onClick={() => setCompletionTarget(undefined)}>Cancel</button>
                      </div>
                    </form>
                  )}
                  {item.status === "completed" && (
                    <div className="result-block">
                      <p><strong>Observed:</strong> {item.actual_result}</p>
                      <p><strong>Learning:</strong> {item.learning}</p>
                    </div>
                  )}
                </article>
              ))}
            </div>
          )}
        </Panel>
      </div>
    </div>
  );
}
