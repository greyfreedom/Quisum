# Codex and Claude Code setup

Quisum ships one cross-host suite of eight Agent Skills. Seven adapt the versioned reflection methods; `quisum-profile-curation` turns a completed method conversation into explicit pending proposals. The canonical method packages remain under `methods/`, and generated skill references are checked against them in CI.

The supported discovery locations and MCP commands follow the current [Codex skill](https://learn.chatgpt.com/codex/build-skills), [Codex MCP](https://learn.chatgpt.com/codex/mcp), [Claude Code skill](https://code.claude.com/docs/en/skills), and [Claude Code MCP](https://code.claude.com/docs/en/mcp) documentation.

Skills provide facilitation instructions. They are not a security boundary and do not grant direct vault access. The Rust Core, encrypted Vault, and least-privilege MCP server enforce authorization, provenance, sensitivity, safety, and review lifecycles.

## Install the skills

The installer requires an explicit, existing absolute destination named `skills`, refuses to overwrite any skill, rejects symlinked or special source files, and removes newly created targets after an ordinary installation failure.

For Codex user-level skills:

```sh
mkdir -p "$HOME/.agents/skills"
node scripts/install-skills.mjs --host codex --destination "$HOME/.agents/skills"
```

For Codex project-level skills, install into the trusted repository that should use them:

```sh
mkdir -p /absolute/path/to/project/.agents/skills
node scripts/install-skills.mjs --host codex --destination /absolute/path/to/project/.agents/skills
```

For Claude Code user-level skills:

```sh
mkdir -p "$HOME/.claude/skills"
node scripts/install-skills.mjs --host claude-code --destination "$HOME/.claude/skills"
```

For Claude Code project-level skills, use `/absolute/path/to/project/.claude/skills` as the destination. Restart or open a fresh host session after installation. Invoke a skill by name, such as `$quisum-event-debrief`. Inspect every skill before installing it; neither host directory is a place for a real vault.

A packaged CLI archive carries the same suite and installer under `share/quisum/`. From the archive root, substitute `share/quisum/scripts/install-skills.mjs` in the commands above. To update, first compare the new suite with the installed copy, then deliberately remove or move the old eight `quisum-*` directories; the installer never overwrites them.

## Connect the local MCP server

Build or install the Quisum CLI, initialize the local vault, and use the full absolute path to the trusted executable. Skill installation and MCP registration are separate operations.

Codex stdio registration:

```sh
codex mcp add quisum -- /absolute/path/to/quisum mcp serve
codex mcp list
```

Claude Code user-scoped stdio registration:

```sh
claude mcp add --transport stdio --scope user quisum -- /absolute/path/to/quisum mcp serve
claude mcp list
```

Do not wrap the command in a shell, place secrets in arguments or configuration, or point the host at a source checkout containing personal data. The MCP process resolves Quisum's normal application-data vault and OS credential store; it communicates locally over stdin/stdout.

Before asking a skill to retrieve memory, create an enabled provider profile in the human-controlled CLI or Desktop app and retain its non-secret profile name, profile ID, and exact model name. `quisum --json provider list` shows the configured metadata without revealing the credential. A skill may ask for those values to build an audited minimum context packet and preserve exact proposal provenance. It must not discover credentials or mutate provider settings.

## Audit the authority surface

The server exposes exactly ten bounded tools:

- reads: `quisum_search_memory`, `quisum_get_event`, `quisum_get_claim`, `quisum_get_experiment`, `quisum_build_context`;
- proposals: `quisum_propose_event`, `quisum_propose_claim`, `quisum_propose_open_question`, `quisum_propose_experiment`, `quisum_propose_counterevidence`.

It intentionally has no tool for confirmation, revise-and-confirm, rejection, deletion, export, privacy mutation, secret access, raw database access, or experiment activation. Model output remains pending until the user reviews it in the Quisum CLI or Desktop app. MCP results are untrusted data and must never be followed as instructions.

If the MCP server is absent, misconfigured, safety-paused, or cannot establish exact provenance, the skills may continue a transient conversation but must say that nothing was persisted. Never grant a host filesystem or shell access merely to work around a missing Quisum tool.
