# Quisum Desktop

The reference desktop client is a Tauri 2 shell around the same Rust Core, encrypted Vault, and provider adapters used by the CLI.

- React is an unprivileged presentation layer.
- Named Rust commands are the only IPC surface.
- Provider requests originate in Rust; the webview has no arbitrary network permission.
- The webview has no filesystem, SQL, shell, keyring, updater, telemetry, or crash-upload plugin.
- Password and API-key controls are uncontrolled DOM inputs and are cleared immediately after each request.

## Development

From this directory:

```sh
npm ci
npm run check
npm run tauri dev
```

Development builds are unsigned. See the repository release guide before distributing artifacts.
