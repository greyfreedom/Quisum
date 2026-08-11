import type { PropsWithChildren, ReactNode } from "react";
import type { Sensitivity } from "./types";

export function PageHeader({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow: string;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        <p className="lede">{description}</p>
      </div>
      {action}
    </header>
  );
}

export function Panel({
  title,
  aside,
  children,
  className = "",
}: PropsWithChildren<{ title?: string; aside?: ReactNode; className?: string }>) {
  return (
    <section className={`panel ${className}`.trim()}>
      {(title !== undefined || aside !== undefined) && (
        <div className="panel-heading">
          {title !== undefined && <h2>{title}</h2>}
          {aside}
        </div>
      )}
      {children}
    </section>
  );
}

export function EmptyState({ children }: PropsWithChildren) {
  return <p className="empty-state">{children}</p>;
}

export function InlineNotice({
  tone = "neutral",
  children,
}: PropsWithChildren<{ tone?: "neutral" | "warning" | "danger" | "success" }>) {
  return (
    <div className={`notice notice-${tone}`} role={tone === "danger" ? "alert" : "status"}>
      {children}
    </div>
  );
}

export function StatusPill({ value }: { value: string }) {
  return <span className={`status-pill status-${value}`}>{humanize(value)}</span>;
}

export function SensitivityPill({ value }: { value: Sensitivity }) {
  return <span className={`sensitivity sensitivity-${value}`}>{humanize(value)}</span>;
}

export function SensitivitySelect({
  name = "sensitivity",
  defaultValue = "private",
  providerEligibleOnly = false,
}: {
  name?: string;
  defaultValue?: Sensitivity;
  providerEligibleOnly?: boolean;
}) {
  return (
    <select name={name} defaultValue={defaultValue}>
      <option value="normal">Normal</option>
      <option value="private">Private</option>
      <option value="highly_sensitive">Highly sensitive</option>
      {!providerEligibleOnly && <option value="local_only">Local only</option>}
    </select>
  );
}

export function Field({
  label,
  hint,
  children,
}: PropsWithChildren<{ label: string; hint?: string }>) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint !== undefined && <small>{hint}</small>}
    </label>
  );
}

export function Loading() {
  return <p className="loading" aria-live="polite">Loading local data…</p>;
}

export function formatDate(value?: string): string {
  if (value === undefined) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString();
}

export function humanize(value: string): string {
  return value.replaceAll("_", " ").replace(/^./, (character) => character.toUpperCase());
}

export function lines(value: FormDataEntryValue | null): string[] {
  if (typeof value !== "string") return [];
  return value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function valueOf(form: FormData, name: string): string {
  const value = form.get(name);
  return typeof value === "string" ? value : "";
}
