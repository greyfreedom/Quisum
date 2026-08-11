DROP TRIGGER audit_events_no_update;
DROP TRIGGER audit_events_no_delete;
DROP INDEX audit_events_created_idx;

ALTER TABLE audit_events RENAME TO audit_events_v1;

CREATE TABLE audit_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  actor TEXT NOT NULL CHECK (actor IN ('user', 'model_host', 'system', 'import')),
  action TEXT NOT NULL CHECK (action IN (
    'vault_created', 'vault_unlocked', 'vault_locked', 'migration_applied',
    'session_created', 'event_recorded', 'event_reviewed', 'candidate_proposed',
    'claim_recorded', 'claim_reviewed', 'claim_revised', 'evidence_added',
    'experiment_reviewed', 'correction_recorded',
    'entity_deleted', 'context_built', 'context_approved', 'context_dispatched',
    'export_created', 'import_completed', 'backup_created', 'restore_completed',
    'policy_changed', 'provider_tested'
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

INSERT INTO audit_events(
  sequence, id, actor, action, entity_type, entity_id, details_json, created_at
)
SELECT sequence, id, actor, action, entity_type, entity_id, details_json, created_at
FROM audit_events_v1;

DROP TABLE audit_events_v1;

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
