import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App, { Onboarding, UnlockGate } from "./App";
import { command } from "./api";
import { PrivacyPage } from "./pages/PrivacyPage";
import { ReviewPage } from "./pages/ReviewPage";
import type { AppStatus, ClaimRecord, ContextPacket, EventRecord } from "./types";

vi.mock("./api", () => ({
  command: vi.fn(),
  failureMessage: (error: unknown) => (error instanceof Error ? error.message : "Local command failed"),
  failureGuidance: () => undefined,
}));

const baseStatus: AppStatus = {
  product: "Quisum",
  version: "1.0.0-rc.1",
  vaultExists: false,
  recoveryExists: false,
  unlocked: false,
  vaultPath: "/synthetic/vault.db",
  recoveryPath: "/synthetic/vault.db.recovery.json",
  encrypted: true,
  telemetry: false,
};

describe("secret-bearing gates", () => {
  beforeEach(() => {
    vi.mocked(command).mockReset();
    vi.mocked(command).mockResolvedValue({});
  });

  it("clears recovery passphrases before the create command settles", async () => {
    let settle: (() => void) | undefined;
    vi.mocked(command).mockImplementation(
      () => new Promise((resolve) => {
        settle = () => resolve({});
      }),
    );
    render(<Onboarding status={baseStatus} onChanged={vi.fn(async () => undefined)} />);
    const passphrase = screen.getByLabelText(/^Recovery passphrase/) as HTMLInputElement;
    const confirmation = screen.getByLabelText("Repeat recovery passphrase") as HTMLInputElement;
    fireEvent.change(passphrase, { target: { value: "synthetic passphrase" } });
    fireEvent.change(confirmation, { target: { value: "synthetic passphrase" } });
    fireEvent.click(screen.getByRole("button", { name: "Create local vault" }));

    await waitFor(() => expect(command).toHaveBeenCalledWith("vault_create", expect.any(Object)));
    expect(passphrase).toHaveValue("");
    expect(confirmation).toHaveValue("");
    settle?.();
  });

  it("clears a recovery passphrase before unlock settles", async () => {
    let settle: (() => void) | undefined;
    vi.mocked(command).mockImplementation(
      () => new Promise((resolve) => {
        settle = () => resolve({});
      }),
    );
    render(
      <UnlockGate
        status={{ ...baseStatus, vaultExists: true, recoveryExists: true }}
        onChanged={vi.fn(async () => undefined)}
      />,
    );
    const passphrase = screen.getByLabelText("Recovery passphrase") as HTMLInputElement;
    fireEvent.change(passphrase, { target: { value: "synthetic passphrase" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock with recovery envelope" }));

    await waitFor(() => expect(command).toHaveBeenCalledWith("vault_unlock_recovery", expect.any(Object)));
    expect(passphrase).toHaveValue("");
    settle?.();
  });
});

describe("desktop shell", () => {
  it("exposes all seven product surfaces after unlock", async () => {
    vi.mocked(command).mockImplementation(async (name: string) => {
      if (name === "app_status") return { ...baseStatus, vaultExists: true, recoveryExists: true, unlocked: true };
      return [];
    });
    render(<App />);
    for (const label of ["Explore", "Self Map", "Review", "Timeline", "Reflog", "Experiments", "Privacy & Models"]) {
      expect(await screen.findByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByText("Encrypted vault unlocked")).toBeInTheDocument();
  });
});

describe("provider configuration", () => {
  it("submits the enforced provider boundary even when its selector is disabled", async () => {
    let settle: (() => void) | undefined;
    vi.mocked(command).mockImplementation(async (name: string) => {
      if (name === "provider_list" || name === "method_list") return [];
      if (name === "provider_create") {
        return new Promise((resolve) => {
          settle = () => resolve({});
        });
      }
      return {};
    });
    render(
      <PrivacyPage
        status={{ ...baseStatus, vaultExists: true, recoveryExists: true, unlocked: true }}
        onLocked={vi.fn(async () => undefined)}
      />,
    );

    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Primary OpenAI" } });
    fireEvent.change(screen.getByLabelText(/^Base URL/), { target: { value: "https://api.openai.com/v1" } });
    fireEvent.change(screen.getByLabelText("Model identifier"), { target: { value: "gpt-5" } });
    const credential = screen.getByLabelText(/^API credential/) as HTMLInputElement;
    fireEvent.change(credential, { target: { value: "synthetic-key" } });
    fireEvent.click(screen.getByRole("button", { name: "Add provider" }));

    await waitFor(() => expect(command).toHaveBeenCalledWith(
      "provider_create",
      expect.objectContaining({ kind: "openai", privacyKind: "cloud" }),
    ));
    expect(credential).toHaveValue("");
    settle?.();
  });
});

describe("human review evidence", () => {
  it("shows concrete linked evidence and model provenance before a claim decision", async () => {
    const evidence: EventRecord = {
      id: "event_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "Asked for a pause",
      status: "confirmed",
      recorded_at: "2026-08-12T08:00:00Z",
      facts: ["I asked for ten minutes before answering."],
      interpretations: [],
      emotions: [],
      bodily_responses: [],
      actions: [],
      short_term_outcomes: [],
      long_term_outcomes: [],
      topics: [],
      sensitivity: "private",
      source: { actor: "user" },
    };
    const claim: ClaimRecord = {
      id: "claim_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      kind: "pattern_hypothesis",
      statement: "A brief pause may help in pressured conversations.",
      status: "pending_review",
      contexts: ["pressured conversations"],
      supporting_evidence: [evidence.id],
      counterevidence: [],
      alternative_explanations: ["The other person may have been unusually patient."],
      evidence_grade: "limited",
      sensitivity: "private",
      source: {
        actor: "model",
        method: "pattern-validation@1.0.0",
        provider: "Local model",
        model: "synthetic-model",
        context_packet_id: "context_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      },
      created_at: "2026-08-12T08:00:00Z",
      updated_at: "2026-08-12T08:00:00Z",
    };
    const context: ContextPacket = {
      id: "context_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      purpose: "Test a narrow pattern",
      method: "pattern-validation@1.0.0",
      provider_profile_id: "provider_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      model: "synthetic-model",
      query: "Does a pause help in pressured conversations?",
      query_sensitivity: "private",
      items: [{
        item_type: "event",
        item_id: evidence.id,
        excerpt: "Asked for a pause\nI asked for ten minutes before answering.",
        sensitivity: "private",
        selection_reason: "lexical_match",
      }],
      exclusions: [],
      requires_approval: false,
      created_at: "2026-08-12T08:00:00Z",
      dispatched_at: "2026-08-12T08:00:01Z",
    };
    vi.mocked(command).mockImplementation(async (name: string) => {
      if (name === "claim_review_queue") return [claim];
      if (name === "event_list") return [evidence];
      if (name === "question_list") return [];
      if (name === "context_detail") return context;
      return {};
    });

    render(<ReviewPage />);
    expect(await screen.findByText("Asked for a pause")).toBeInTheDocument();
    expect(screen.getByText("I asked for ten minutes before answering.")).toBeInTheDocument();
    expect(screen.getByText(/Local model · synthetic-model · pattern-validation@1.0.0 · context/)).toBeInTheDocument();
    expect(screen.getByText("Inspect the exact model context")).toBeInTheDocument();
    expect(screen.getByText((_, element) => (
      element?.tagName === "BLOCKQUOTE"
      && element.textContent === "Asked for a pause\nI asked for ten minutes before answering."
    ))).toBeInTheDocument();
  });
});
