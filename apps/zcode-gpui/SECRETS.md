# Secrets vault (GPUI only)

Project secrets (API tokens, upload keys, …) live in the operating system's
secret store instead of `.env` files. GPUI owns the store; agents only see the
values in memory and inject them into the processes they start.

## Product rules

- **Storage.** Values are kept in the native OS store through the `keyring`
  crate (v4, `v1` mode): Windows Credential Manager, macOS Keychain, Linux
  Secret Service. Service name `GCode`, account `secret:<NAME>`. Fixed names,
  so rebuilding or reinstalling GCode finds the same entries. (macOS may ask
  once to allow an unsigned rebuild.)
- **Index.** The OS store cannot be listed, so GPUI keeps the list of names and
  their workspace scope in `~/.zcode/v2/gpui_secrets.json` (no values, ever).
  A name in the index whose OS entry is missing is shown as "missing" and never
  injected.
- **Names.** `^[A-Za-z_][A-Za-z0-9_]*$`, at most 128 characters. Reserved and
  rejected (UI and protocol): `PATH`, `HOME`, `USERPROFILE`, `SHELL`, `TEMP`,
  `TMP`, `TMPDIR`, `COMSPEC`, `SYSTEMROOT`, `PATHEXT`, `NODE_OPTIONS`,
  `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, proxy variables
  (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, any case), and any
  `ZCODE_*` / `GCODE_*` name. Windows names compare case-insensitively.
- **Values** are write-only in the UI: after saving, a value can be replaced or
  deleted but not shown again. Empty values are rejected.
- **Scope.** A secret with no workspaces applies to every workspace. Otherwise
  it applies only to the listed workspace keys (the canonical workspace path
  GPUI already uses as `WorkspaceHandle::key`).
- **Injection.** Into every Bash/tool subprocess and every stdio MCP server
  started by the workspace agent. Requests with `env.base = "empty"` (a
  deliberately clean environment) get nothing. An MCP server's explicit
  `config.env` wins over a secret of the same name.
- **Redaction.** Before a tool result reaches the model, the transcript, the
  artifact store or the UI, every occurrence of a secret value is replaced with
  `***`. Live tool progress events (Bash output preview, background task
  output, MCP progress) are redacted the same way before they are stored or
  streamed. Values shorter than 6 characters are not redacted (too many false
  hits); the UI warns when saving one.
- The phone never holds or receives secrets; it does not call `secrets/sync`.

## Owners and event order

```
OS secret store (values)          ~/.zcode/v2/gpui_secrets.json (names + scope)
         \                          /
          GPUI SecretVault (AppState.secrets) — single writer
              | 1. startup: background read of every indexed value
              | 2. startup/storageState=ready (each (re)connect of a workspace agent)
              | 3. after every add / replace / delete / scope change
              v  secrets/sync { workspace, secrets: [{name, value}] }  replace-all
     workspace agent: core secret vault (process memory only)
              |-- Bash/tool env: create-app wraps the session ExecutionPort,
              |   values read per command (an edit applies to the next command)
              |-- stdio MCP env: read when the server process starts
              '-- redaction: serializeOutput + appendEvent(ToolCallProgress)
```

- GPUI runs one agent process per workspace, so GPUI filters by scope and the
  agent keeps a single process-wide list. `secrets/sync` is idempotent
  (replace-all); the latest request wins.
- If an agent restarts, its list is empty until the next `ready`, which
  re-sends it. A sync that arrives before the startup read finished is skipped;
  the read's completion sends to every started workspace.
- Running MCP servers keep the environment they were started with; a changed
  value reaches them at their next start (new session or MCP reconnect).
- The agent never persists or logs values; logs carry the count only.

## Protocol

`secrets/sync` (client → agent, GPUI only; Electron never calls it; an old
agent answers method-not-found and GPUI shows the vault as unavailable for
that workspace):

```json
{ "workspace": { "workspacePath": "…", "workspaceKey": "…" },
  "secrets": [{ "name": "STEAM_TOKEN", "value": "…" }] }
```

Result: `{ "count": <number of secrets now active> }`. Invalid or reserved
names, empty values, duplicates and more than 256 entries reject the whole
request (invalid params); the previous list stays active.

## Limits (stated to the user in Settings)

- Any program running as the same OS user can read Credential Manager entries,
  including commands the agent itself runs. Redaction keeps values out of the
  transcript and the model's view; it is not a sandbox.
- Transcripts written before a secret was added are not rewritten.
- Encoded forms of a value (base64, URL-escaped) are not redacted.

## Acceptance scenarios

1. Add `DEMO_TOKEN` in Settings › Secrets → the entry `GCode / secret:DEMO_TOKEN`
   exists in the OS store; `gpui_secrets.json` holds the name only.
2. In a workspace in scope, Bash `echo $DEMO_TOKEN` shows `***` in the tool
   output, the transcript and the model request; `[ -n "$DEMO_TOKEN" ] && echo set`
   prints `set`.
3. In a workspace out of scope, the same check prints nothing.
4. Rebuild and relaunch GCode → the secret is still listed and still injected.
5. Delete it → the OS entry is gone and the next Bash command no longer has it.
6. Restart the workspace agent (or reconnect) → the secret is injected again
   without opening Settings.
