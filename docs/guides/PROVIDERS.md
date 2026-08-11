# Provider configuration

Quisum works without a provider for capture, review, search, export, backup, and recovery. A configured provider receives only a purpose-built context packet that has passed local policy.

## Trust model

A provider profile is a local declaration, not proof of vendor behavior. It records protocol kind, base URL, model identifier, cloud/local boundary, supported capabilities, retention notes, and which sensitivity levels the user permits. `local_only` data is never eligible for provider egress. `highly_sensitive` context requires an exact, one-time packet approval.

Cloud profiles require HTTPS and reject loopback/private hosts. `local_http` profiles require a syntactic loopback host such as `127.0.0.1`, `[::1]`, or `localhost`. HTTP clients reject redirects, ignore ambient proxy settings, bound response sizes and timeouts, and redact request/response bodies from errors.

## Add a profile

These base URLs are protocol roots; the adapter adds the named operation path.

| Kind | Example base URL | Operation path |
| --- | --- | --- |
| `openai` | `https://api.openai.com/v1` | `/responses` |
| `anthropic` | `https://api.anthropic.com/v1` | `/messages` |
| `openai_compatible` | vendor-specific HTTPS `/v1` root | `/chat/completions` |
| `local_http` | `http://127.0.0.1:11434/v1` | `/chat/completions` |

For a cloud profile, let Quisum prompt for the credential:

```sh
quisum provider add \
  --name "Personal provider" \
  --kind openai \
  --base-url https://api.openai.com/v1 \
  --model <model-id> \
  --privacy cloud \
  --trust normal \
  --trust private \
  --streaming \
  --json-schema \
  --with-secret
```

Never put an API key in a command argument, URL, environment committed to source, screenshot, or issue. `--secret-stdin` is available for a controlled secret pipe. The value goes directly to the OS credential store; only an opaque reference enters the vault.

Capabilities are declarations. Enable `--streaming`, `--json-schema`, `--tool-use`, or `--vision` only when the exact endpoint/model supports them. V1 method execution uses text/streaming and bounded structured output; tool and vision declarations do not grant extra local authority.

For an unauthenticated local endpoint:

```sh
quisum provider add \
  --name "Local model" \
  --kind local_http \
  --base-url http://127.0.0.1:11434/v1 \
  --model <local-model-id> \
  --privacy local \
  --trust normal \
  --trust private \
  --streaming
```

## Test without personal context

List profiles to obtain the canonical provider ID, then run the content-free connection test:

```sh
quisum provider list
quisum provider test <provider-id>
```

The test requests the provider's models endpoint and records only success plus latency. It does not submit a conversation. A successful connection does not establish model quality, retention behavior, or safety.

## Preview and send exact context

Build an immutable packet first:

```sh
quisum context build \
  --provider <provider-id> \
  --purpose free-reflection \
  --method free-reflection@1.0.0 \
  --query "What is one alternative explanation for this event?"
```

Inspect its included and excluded records. If it contains highly sensitive material, approve that exact packet once:

```sh
quisum context approve <context-id>
```

Then send it:

```sh
quisum provider chat --context <context-id> --stream
```

Dispatch atomically rechecks the provider identity, enablement, trust, query sensitivity, selected records, exact excerpts, and approval. A changed/deleted record or profile blocks the call rather than silently changing the reviewed packet.

## Revoke or rotate

```sh
quisum provider revoke-secret <provider-id>
```

Revocation disables the profile before deleting the OS credential. Historical provenance remains, but it cannot authorize new calls. Because endpoint, protocol, model, and credential reference are immutable identity fields, create a new profile for a rotated identity rather than rewriting history.

See the evidence-based [provider compatibility matrix](../PROVIDER_COMPATIBILITY.md). No provider is certified merely because its protocol fixture passes.
