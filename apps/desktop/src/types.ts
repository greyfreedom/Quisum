export type Sensitivity =
  | "normal"
  | "private"
  | "highly_sensitive"
  | "local_only";

export interface AppStatus {
  product: string;
  version: string;
  vaultExists: boolean;
  recoveryExists: boolean;
  unlocked: boolean;
  vaultPath: string;
  recoveryPath: string;
  encrypted: boolean;
  telemetry: boolean;
}

export interface Source {
  actor: "user" | "model" | "system" | "import";
  method?: string;
  provider?: string;
  model?: string;
  context_packet_id?: string;
  import_id?: string;
}

export interface EventRecord {
  id: string;
  title: string;
  status: "proposed" | "confirmed" | "rejected" | "deleted";
  occurred_at?: string;
  recorded_at: string;
  facts: string[];
  interpretations: string[];
  emotions: string[];
  bodily_responses: string[];
  actions: string[];
  short_term_outcomes: string[];
  long_term_outcomes: string[];
  topics: string[];
  sensitivity: Sensitivity;
  source: Source;
}

export interface ClaimRecord {
  id: string;
  kind: string;
  statement: string;
  status: string;
  contexts: string[];
  supporting_evidence: string[];
  counterevidence: string[];
  alternative_explanations: string[];
  evidence_grade: string;
  user_endorsement?: string;
  sensitivity: Sensitivity;
  source: Source;
  created_at: string;
  updated_at: string;
  revisit_after?: string;
}

export interface OpenQuestion {
  id: string;
  question: string;
  status: "proposed" | "open" | "answered" | "closed" | "deleted";
  contexts: string[];
  related_claims: string[];
  sensitivity: Sensitivity;
  created_at: string;
  source: Source;
}

export interface Experiment {
  id: string;
  title: string;
  hypothesis: string;
  prediction: string;
  action: string;
  stop_condition: string;
  risk_level: "low" | "moderate";
  status: "proposed" | "active" | "completed" | "cancelled" | "deleted";
  related_claims: string[];
  planned_at: string;
  completed_at?: string;
  expected_result?: string;
  actual_result?: string;
  learning?: string;
  sensitivity: Sensitivity;
  source: Source;
}

export interface MethodSummary {
  id: string;
  title: string;
  description: string;
  reviewStatus: "unreviewed" | "internal" | "professional";
  requires: {
    streaming: boolean;
    structured_output: boolean;
    tool_use: boolean;
  };
  valid: boolean;
}

export interface ProviderProfile {
  id: string;
  name: string;
  kind: "openai" | "anthropic" | "openai_compatible" | "local_http";
  base_url: string;
  model: string;
  privacy_kind: "cloud" | "local";
  capabilities: {
    streaming: boolean;
    tool_use: boolean;
    json_schema: boolean;
    vision: boolean;
    context_limit?: number;
  };
  trusted_sensitivities: Sensitivity[];
  credential_ref?: string;
  retention_note?: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface ContextItem {
  item_type: string;
  item_id: string;
  excerpt: string;
  sensitivity: Sensitivity;
  selection_reason: string;
}

export interface ContextPacket {
  id: string;
  purpose: string;
  method: string;
  provider_profile_id: string;
  model: string;
  query: string;
  query_sensitivity: Sensitivity;
  items: ContextItem[];
  exclusions: Array<{ item_id: string; reason: string }>;
  requires_approval: boolean;
  approved_at?: string;
  created_at: string;
  dispatched_at?: string;
}

export interface ReflectionSession {
  id: string;
  method: string;
  title?: string;
  status: "active" | "completed" | "abandoned" | "deleted";
  started_at: string;
  completed_at?: string;
  sensitivity: Sensitivity;
}

export interface SessionMessage {
  id: string;
  session_id: string;
  role: "user" | "assistant";
  content: string;
  created_at: string;
  sensitivity: Sensitivity;
  source: Source;
}

export interface SessionDetail {
  session: ReflectionSession;
  messages: SessionMessage[];
}

export interface SearchResult {
  itemType: string;
  itemId: string;
  excerpt: string;
  sensitivity: Sensitivity;
  selectionReason: string;
  relevanceScore: number;
  recordedAt: string;
}

export interface ReflogEntry {
  sequence: number;
  event: {
    id: string;
    actor: string;
    action: string;
    entity_type?: string;
    entity_id?: string;
    created_at: string;
  };
}
