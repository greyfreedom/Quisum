CREATE TABLE vault_metadata (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE sessions (
  id TEXT PRIMARY KEY NOT NULL,
  method TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active', 'completed', 'abandoned', 'deleted')),
  started_at TEXT NOT NULL,
  completed_at TEXT,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE messages (
  id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  created_at TEXT NOT NULL,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX messages_session_created_idx ON messages(session_id, created_at);

CREATE TABLE session_summaries (
  id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  review_status TEXT NOT NULL CHECK (review_status IN ('pending_review', 'confirmed', 'rejected')),
  created_at TEXT NOT NULL,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE events (
  id TEXT PRIMARY KEY NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('proposed', 'confirmed', 'rejected', 'deleted')),
  title TEXT NOT NULL,
  occurred_at TEXT,
  recorded_at TEXT NOT NULL,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX events_status_recorded_idx ON events(status, recorded_at DESC);

CREATE TABLE claims (
  id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN (
    'explicit_fact', 'user_self_assessment', 'current_state', 'value',
    'strength', 'need_or_boundary', 'pattern_hypothesis', 'confirmed_insight'
  )),
  status TEXT NOT NULL CHECK (status IN (
    'proposed', 'pending_review', 'confirmed', 'revised', 'uncertain',
    'rejected', 'contradicted', 'superseded', 'deprecated', 'deleted'
  )),
  statement TEXT NOT NULL,
  evidence_grade TEXT NOT NULL CHECK (evidence_grade IN ('limited', 'moderate', 'repeated', 'contested')),
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  revisit_after TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX claims_status_updated_idx ON claims(status, updated_at DESC);
CREATE INDEX claims_kind_status_idx ON claims(kind, status);

CREATE TABLE evidence_links (
  claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  evidence_id TEXT NOT NULL,
  relation TEXT NOT NULL CHECK (relation IN ('supporting', 'counter')),
  review_status TEXT NOT NULL CHECK (review_status IN ('proposed', 'confirmed', 'rejected')),
  source_actor TEXT NOT NULL CHECK (source_actor IN ('user', 'model_host', 'system', 'import')),
  context_packet_id TEXT REFERENCES context_packets(id),
  created_at TEXT NOT NULL,
  PRIMARY KEY (claim_id, evidence_id, relation)
) WITHOUT ROWID, STRICT;
CREATE INDEX evidence_source_idx ON evidence_links(evidence_id, review_status);

CREATE TABLE claim_revisions (
  id TEXT PRIMARY KEY NOT NULL,
  claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  actor TEXT NOT NULL,
  reason TEXT NOT NULL,
  before_json TEXT NOT NULL CHECK (json_valid(before_json)),
  after_json TEXT NOT NULL CHECK (json_valid(after_json)),
  created_at TEXT NOT NULL
) STRICT;
CREATE INDEX claim_revisions_claim_created_idx
  ON claim_revisions(claim_id, created_at);

CREATE TRIGGER claim_revisions_no_update
BEFORE UPDATE ON claim_revisions
BEGIN
  SELECT RAISE(ABORT, 'claim revisions are append-only');
END;

CREATE TABLE user_reviews (
  id TEXT PRIMARY KEY NOT NULL,
  claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  decision TEXT NOT NULL,
  reviewed_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX user_reviews_claim_created_idx ON user_reviews(claim_id, reviewed_at);

CREATE TRIGGER user_reviews_no_update
BEFORE UPDATE ON user_reviews
BEGIN
  SELECT RAISE(ABORT, 'user reviews are append-only');
END;

CREATE TABLE user_corrections (
  id TEXT PRIMARY KEY NOT NULL,
  active INTEGER NOT NULL CHECK (active IN (0, 1)),
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX corrections_active_updated_idx
  ON user_corrections(active, updated_at DESC);

CREATE TABLE open_questions (
  id TEXT PRIMARY KEY NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('proposed', 'open', 'answered', 'closed', 'deleted')),
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  created_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX open_questions_status_created_idx
  ON open_questions(status, created_at DESC);

CREATE TABLE experiments (
  id TEXT PRIMARY KEY NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('proposed', 'active', 'completed', 'cancelled', 'deleted')),
  risk_level TEXT NOT NULL CHECK (risk_level IN ('low', 'moderate')),
  planned_at TEXT NOT NULL,
  completed_at TEXT,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  crisis_related INTEGER NOT NULL CHECK (crisis_related IN (0, 1)),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX experiments_status_planned_idx
  ON experiments(status, planned_at DESC);

CREATE TABLE assessments (
  id TEXT PRIMARY KEY NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('completed', 'superseded', 'deleted')),
  administered_at TEXT NOT NULL,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'private', 'highly_sensitive', 'local_only')),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE context_packets (
  id TEXT PRIMARY KEY NOT NULL,
  provider_profile_id TEXT NOT NULL,
  purpose TEXT NOT NULL,
  requires_approval INTEGER NOT NULL CHECK (requires_approval IN (0, 1)),
  approved_at TEXT,
  created_at TEXT NOT NULL,
  dispatched_at TEXT,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;
CREATE INDEX context_packets_created_idx ON context_packets(created_at DESC);

CREATE TABLE provider_profiles (
  id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('openai', 'anthropic', 'openai_compatible', 'local_http')),
  privacy_kind TEXT NOT NULL CHECK (privacy_kind IN ('cloud', 'local')),
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE consent_records (
  id TEXT PRIMARY KEY NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('grant', 'revoke', 'grant_once')),
  created_at TEXT NOT NULL,
  expires_at TEXT,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE TABLE audit_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  actor TEXT NOT NULL CHECK (actor IN ('user', 'model_host', 'system', 'import')),
  action TEXT NOT NULL CHECK (action IN (
    'vault_created', 'vault_unlocked', 'vault_locked', 'migration_applied',
    'session_created', 'event_recorded', 'event_reviewed', 'candidate_proposed',
    'claim_recorded', 'claim_reviewed', 'claim_revised', 'evidence_added',
    'experiment_reviewed',
    'correction_recorded',
    'entity_deleted', 'context_built', 'context_approved', 'context_dispatched',
    'export_created', 'backup_created', 'restore_completed', 'policy_changed',
    'provider_tested'
  )),
  entity_type TEXT CHECK (entity_type IS NULL OR entity_type IN (
    'vault', 'session', 'message', 'session_summary', 'event', 'claim',
    'revision', 'review', 'correction', 'open_question', 'experiment',
    'context_packet', 'provider', 'consent', 'audit', 'assessment',
    'export', 'backup', 'import'
  )),
  entity_id TEXT,
  details_json TEXT NOT NULL CHECK (json_valid(details_json)),
  created_at TEXT NOT NULL
) STRICT;
CREATE INDEX audit_events_created_idx ON audit_events(created_at);

CREATE TRIGGER audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
  SELECT RAISE(ABORT, 'audit events are append-only');
END;

CREATE TRIGGER audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
  SELECT RAISE(ABORT, 'audit events are append-only');
END;

CREATE TABLE embeddings (
  item_id TEXT NOT NULL,
  item_type TEXT NOT NULL,
  model_id TEXT NOT NULL,
  dimensions INTEGER NOT NULL CHECK (dimensions > 0),
  vector BLOB NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (item_id, model_id)
) WITHOUT ROWID, STRICT;

CREATE VIRTUAL TABLE events_fts USING fts5(
  id UNINDEXED,
  title,
  facts,
  interpretations,
  topics,
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE claims_fts USING fts5(
  id UNINDEXED,
  statement,
  contexts,
  alternatives,
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE corrections_fts USING fts5(
  id UNINDEXED,
  statement,
  topics,
  tokenize = 'unicode61 remove_diacritics 2'
);
