# Privacy model

## Defaults

Quisum has no required account, remote service, telemetry, crash upload, or background synchronization. The encrypted vault is the system of record. API credentials are held by an OS credential facility or a password-protected secret store, never ordinary configuration or logs.

## Sensitivity

Every stored item has one of four levels:

- `normal`: eligible for relevant retrieval.
- `private`: eligible only when relevant to the current purpose.
- `highly_sensitive`: requires an explicit policy grant and is shown in context preview.
- `local_only`: never leaves the local process through a provider adapter.

Topic policies can further limit family, intimate relationships, work, physical health, finances, and trauma-related content.

## Context packets

Every outbound model request references an immutable context packet containing:

- declared purpose and active method;
- provider profile and model identifier;
- selected item IDs, excerpts, sensitivity, and reason for selection;
- items intentionally excluded and the policy reason;
- approval status when highly sensitive content is present;
- creation and dispatch timestamps.

Selection is deny-by-default. `local_only`, rejected, deleted, crisis-related, and unrelated highly sensitive entries are ineligible. User corrections are considered before candidate claims.

## Provider metadata

Provider profiles state cloud or local operation, base URL, supported capabilities, retention notes supplied by the user or adapter, and whether the endpoint is trusted for private or highly sensitive content. Quisum does not guarantee a provider's behavior; it makes the disclosure visible and enforceable locally.

## Logs and diagnostics

Application logs use stable IDs, operation names, timings, and redacted error categories. They do not contain message bodies, claim statements, keys, authorization headers, database keys, or exported data. Diagnostics and crash reports are local and opt-in for sharing.

## Export, backup, and deletion

- Plaintext exports require an explicit user action and a destination warning.
- Backups are encrypted by default and verified before being declared usable.
- Full deletion requires vault unlock, an explicit confirmation, and best-effort removal of backups chosen by the user.
- Derived claims are re-evaluated when sources are removed.
- SQLCipher protects data at rest while locked; it does not protect an unlocked process, screenshots, swap configured by the OS, compromised endpoints, or a malicious provider receiving approved context.

## Repository safety

Initialization detects Git worktrees and refuses to place a real vault within one unless the user gives an explicit override after a prominent warning. Source fixtures remain synthetic.
