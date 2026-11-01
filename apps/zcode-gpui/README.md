# zcode-gpui

> **Unofficial.** This is a community GPUI client for the ZCode CLI backend. It is not an official ZCode product and is not supported by the ZCode team.

Native Rust/GPUI frontend for the existing ZCode CLI app-server. Chat, project/session navigation, model and mode selection, basic review/files/terminal panes, a first Settings slice including API-key provider management, Browser Use through your own Helium/Chrome, and agents that speak to you on the PC are implemented. This is **not complete Electron parity**: account-sign-in providers, capability configuration, parts of plugin/workflow management, host-owned task metadata, and native lifecycle/distribution remain incomplete. See [PARITY.md](PARITY.md) and [SETTINGS.md](SETTINGS.md).

## Running

```sh
cargo run --release --manifest-path apps/zcode-gpui/Cargo.toml -- --workspace <absolute_project_path>
```

GPUI and `gpui_platform` are git-pinned to Zed revision `1a28cff4b409169bac058bca40dfbfeb7621d19b`; Ely components are vendored at `2f8b2f687cd1e9b98a7cd29d4e882d09406fc547` in `vendor/ely-gpui-component`, because their git repository is no longer reachable. The crate uses Rust edition 2024. Dependency versions and platform features are authoritative in `Cargo.toml` and `Cargo.lock`.

## Implemented surfaces

- **Projects and sessions:** Lists every local project from the bootstrap settings file (last active first, no cap; the launch directory is not a project unless passed as an argument). Right-click a project for New task, Open in File Explorer, Open in editor, Copy path and Remove from sidebar. Backends spawn lazily; idle, non-running workspaces unload after 10 minutes by default (`ZCODE_GPUI_IDLE_SECS`). Windows Job Objects and quit cleanup own child-process termination. The newest 25 tasks of each workspace and the open folders are cached in `~/.zcode/v2/gpui_sidebar_cache.json`, so the sidebar is filled at startup. The Tasks agent and the agents of folders left open start in the background and preload their 3 newest tasks. The agent's live list replaces the cached rows. Projects are ordered by their latest task activity, held still while a task runs. Sidebar lists load progressively; search filters loaded session titles, not a complete host task index.
- **Chat:** Markdown, highlighted code, diffs, a virtualized transcript, model/thinking/mode selection, turn metadata, plans, inline permissions/questions, and supported edit/retry/undo/session actions.
- **Composer and queue:** Multi-line input, local-path attachments, task/skill/file/plugin mentions shown as chips (also in sent messages; the sent text is the same markdown as Electron's), `@` file search over a background workspace index (honours ignore files, no depth limit, skips Unity/build folders and `*.meta`, fzf-style ranking; see [COMPOSER_PARITY.md](COMPOSER_PARITY.md)), live scoped skill/plugin references and CLI-owned slash catalogs (including `/init` and custom commands), local `/plan` draft/submission routing, aggregate background activity and read-only child-agent chats, queue Edit/reorder/send-now/delete/auto-drain, and availability-gated goal Pause/Resume. Ctrl+Enter (Cmd+Enter on macOS) and running modifier-click on Send choose one-shot delivery without changing follow-up preference. Paused plain-input and new-goal queues offer Clear/Keep/Cancel with reviewed-ID and stale-callback guards; the CLI alone changes accepted queue state. Drafts live in memory; they are **not saved across process restarts**. Existing-chat clipboard images use bounded begin/chunk/commit uploads with Retry/Cancel and blocked submission until ready; new-chat image pastes retain format-correct owned temporary paths. General new-chat non-path upload remains incomplete.
- **Safe submission:** Failed enqueue preserves text, attachments, and temporary-file ownership. Explicit rejection restores the originating draft without overwriting newer text or an Edit/Rename operation. An uncertain acknowledgement is reported without automatic resend. A delayed create acknowledgement cannot steal newer navigation.
- **Tool panes:** Basic Git review with stage/commit/push and diffs, a lazy file tree with text/Markdown and bounded static PNG/JPEG/GIF/WebP/BMP/ICO previews, and a local PTY terminal with scrollback drag selection and Ctrl/Cmd+C copy (Ctrl+C without selection still interrupts). Usage, MCP status listing/Open authorization, plugins, workflows, and subagent views also exist; OAuth opening is owner/connection/query guarded and uses a secret-safe native adapter, not MCP configuration CRUD. The presence of a pane does not imply every management action is implemented.
- **Plugins and workflows:** Workspace-scoped install/update/enable/uninstall/restore/source management uses checked results and shared pending guards. Personal Source plugin cards expose draft-safe Example Prompt prefill with canonical enabled references; an existing target draft blocks prefill rather than being overwritten. Saved project/global workflows have typed arguments, Run in new chat, definition inspection, saved-run history/Open chat, eligible-run model/concurrency settings, metadata editing, and confirmed Delete/global-to-project Move. Writes preflight the inspected definition and preserve newer edits; existing move destinations are never overwritten. Definitions and scheduling stay backend-owned; unknown outcomes require Reload, not automatic replay or deletion. Read-only plugin Details/Refresh and typed user/workspace configuration are reachable, with masked secret drafts, explicit per-option Clear and confirmed scope-correct Reset. Scoped writes preflight current CLI values, preserve unspecified/newer edits and refresh only their origin. Projected Open run/Open successor and All runs are reachable; a focused amendment follows its exact returned run/tool-call pair only after the authoritative stream contains it. Focused run details show journal-owned artifact metadata with explicit Refresh and distinct loading/empty/unavailable/error states. Markdown deliverables offer bounded latest-version View/Retry/Close through the authorized CLI read, with passive links/images and strict complete-response validation. Store paths are never opened. Other artifact formats, version navigation, cross-project run detail and native lifecycle evidence remain incomplete.
- **Browser Use:** The agent's browser tool drives tabs in your own Helium or Google Chrome through the ZCode extension (`apps/zcode-browser-extension`) and a small native host (`zcode-browser-host`) that relays to a token-protected loopback bridge in GPUI. Agent tabs open in the background inside a "ZCode" tab group; the agent can also claim and hand back your own tabs. Settings › Browser Use shows the connection, registers the native host (Windows only for now) and opens the extension folder. Recording, downloads and file uploads are not supported yet. See [BROWSER_BRIDGE.md](BROWSER_BRIDGE.md).
- **Voice:** With Settings › Voice › Let agents speak on, the main agent gets a `Speak` tool and says one short sentence on this PC when it needs you, finishes a task or has a plan ready. GPUI says "Permission needed", "Question waiting" or "Plan ready" itself when such a prompt appears and the agent hasn't spoken that turn. Speech is generated locally by the separate `zcode-voice` program (Kokoro through kokoro-tiny), which downloads its ~350 MB model to `~/.zcode/v2/voice` on first use and exits 30 seconds after its last line finishes (a restart takes about 0.6 s). Voice, speed, volume and "only when ZCode isn't focused" are configurable; Test voice works while it is off. Turning it on applies to new or resumed sessions. The phone never speaks and never receives speak requests. See [VOICE.md](VOICE.md).
- **Secrets:** Settings › Secrets saves a name and value to the OS secret store (`keyring` crate: Windows Credential Manager, macOS Keychain, Linux Secret Service; service `GCode`, account `secret:<NAME>`). Values are write-only in the UI, and only names and workspace scope go to `~/.zcode/v2/gpui_secrets.json`. Each workspace agent receives its in-scope secrets with `secrets/sync` when it becomes ready and after every edit. The agent injects them into Bash (from the next command) and stdio MCP servers (at their next start) and replaces their values with `***` in tool output. The phone never holds secrets. See [SECRETS.md](SECRETS.md).
- **Settings:** The sidebar gear, command center, and `CmdOrCtrl+,` open the same per-window destination. General has System/English/Chinese language; Appearance has System/light/dark theme and 12–20 px interface font size; Shortcuts supports the commands listed below. Memory persists `memoryEnabled` (default off) and supplies its committed value to CLI runtime-preference requests; it permits configured memory for newly materialized/resumed runtimes, not an active-runtime reset. Back/Escape preserve the workspace, session, and composer. Streams continue while Settings is open. Localization outside Settings and selected primary surfaces is still partial.

### Settings persistence and paths

One process-global preference controller serializes background read–modify–write mutations, including recent-project updates. It preserves unknown JSON fields and publishes runtime changes only after a successful disk write. Corrupt/unreadable settings and a running Electron desktop cause refusal rather than overwrite; errors are visible. Raw “Open settings file” is an advanced editor action, not a safe coexistence writer.

- Bootstrap file: `<settings-home>/.zcode/v2/setting.json`.
- Settings home: nonblank `ZCODE_DESKTOP_HOME_DIR`, otherwise `HOME`, then `USERPROFILE`.
- Application data base: persisted `dataBaseDir`, then `ZCODE_DATA_BASE_DIR`, then default home. The data root appends `.zcode`.
- `ZCODE_DESKTOP_HOME_DIR` changes the bootstrap location, not the default data base.
- No automatic migration or deletion of existing user data is performed.

Theme preference System remains distinct from the effective OS appearance. Ely controls and legacy semantic colors/interface typography update together. Code and terminal body sizes remain independent. Preference persistence/re-hydration is tested with a new application owner; an actual native executable restart has not been recorded.

### Supported application shortcuts

`CmdOrCtrl` means Command on macOS and Control elsewhere. Overrides apply live after successful persistence. Empty bindings disable a command; Reset restores its defaults. Unsupported command IDs in the file are retained but not offered as working controls.

| Action         | Default                            |
| -------------- | ---------------------------------- |
| Settings       | `CmdOrCtrl+,`                      |
| Command center | `CmdOrCtrl+k`, `CmdOrCtrl+Shift+p` |
| Switch theme   | `CmdOrCtrl+Shift+l`                |
| Terminal       | `CmdOrCtrl+j`                      |
| Tool dock      | `CmdOrCtrl+Alt+b`                  |
| New task       | `CmdOrCtrl+n`                      |
| Open workspace | `CmdOrCtrl+o`                      |

## Architecture and protocol

```text
GPUI window / draft / preference projection
    ↔ per-workspace process adapter, stdio NDJSON
    ↔ existing CLI app-server
        owns session state, CommandInbox admission, revisions, and streams
```

The frontend uses the V4 command/topic contract in `packages/shared/src/zcode-protocol-v4/` and physical wire version 3. It validates subscription/epoch/sequence continuity and bounded fragment assembly. Process-generation checks independently reject stale pumps, flow updates, and delayed restarts. A successful enqueue or admission acknowledgement is not a completed model turn.

Desktop live delivery (`desktop-continuous`) and recovery delivery (`web-remote-replayable`) retain distinct semantics against the same backend owner. GPUI does not create a second scheduler, task metadata owner, remote registry, or backend fork. Account-provider headers, official MCP authentication, and host scheduling remain unavailable or explicitly failed fast. Browser execution goes through the extension bridge and fails fast when no browser is connected. Direct API-key providers are the supported model-auth path.

### Source layout

| Slice                            | Responsibility                                                                            |
| -------------------------------- | ----------------------------------------------------------------------------------------- |
| `app/`                           | Root view/state, Settings/navigation, dock and supporting panes                           |
| `backend/`                       | Process adapters, wire assembly, routing, request/ACK correlation and submission recovery |
| `conversation/`                  | Mirrored state, interactions, queue, plans and message actions                            |
| `transcript/`                    | Virtualized list and row rendering                                                        |
| `composer/`                      | Input, attachment tray, mentions and model/mode menus                                     |
| `sessions/`                      | Project/session sidebar                                                                   |
| `review/`, `files/`, `terminal/` | Local tools and views                                                                     |
| `shared/`                        | Preferences, path resolution, theme/i18n, platform adapters and render utilities          |
| `browser_bridge/`, `bin/`        | Browser Use bridge, host registration and the `zcode-browser-host` native host            |
| `remote/`                        | Phone remote control (LAN gateway and phone client)                                       |
| `voice/`                         | Voice service: `interaction/speak`, fallback lines and the `zcode-voice` sidecar process  |
| `secrets/`                       | Secrets vault: OS secret store access and the names/scope index                           |

Slices extend `RootView` and `AppState`. Rust source/test files are capped at 400 lines by `tests/source_limits.rs`. The root TypeScript architecture policy marks this app unmanaged; its successful check is not a Rust ownership audit.

## Backend resolution

1. `ZCODE_GPUI_AGENT_PROGRAM` with optional `ZCODE_GPUI_AGENT_ARGS`. A JSON array of strings is the canonical args format; legacy whitespace splitting is retained. Defaults include `app-server --stdio --surface desktop`.
2. The compiled backend beside the GPUI executable: `zcode-backend[.exe]` plus `provider/zcode-builtin.json` (and optionally `node_modules/ripgrep`), produced by `scripts/build_backend.ts`. Skipped unless the provider release is present. `bun run start:gpui` rebuilds it into `target/release` on every launch (about 4 s). Measured on 2026-10-30, an idle agent used 178 MB compiled against 253 MB from source, and was ready in 0.4 s instead of 0.9 s. Rebuild the CLI packages' `dist/` before launching, because the bundle is built from them. `bun run dev:gpui` still runs the agent from source.
3. Bun plus this branch's CLI source, offered only when its workspace dependencies are installed and built. Bun is found through `BUN_INSTALL`, `PATH`, then `~/.bun/bin`.

GPUI never launches the installed ZCode desktop app (`ZCode.exe`), not for the agent, the Services Host, or plugin MCP servers. It only reads files from that install: its ripgrep and its Built-in Provider release. Plugin MCP servers in the plugin cache shared with Desktop are rebound to the agent's own runtime when they load (`bindPluginHostServersToCurrentRuntime`).

The Services Host used by Settings › Subagents resolves separately:

1. `ZCODE_GPUI_SERVICES_RUNTIME` / `ZCODE_GPUI_SERVICES_BUNDLE`: the built `packages/server/dist/remote/zcode-server.cjs` under an Electron-compatible runtime.
2. Bun plus `scripts/services_host.ts`, which loads the unchanged `packages/server/src/entry-stdio.ts` from source after supplying the build-time version and the production Built-in Provider release (the installed Desktop's copy when present). Needs only `bun install`.
3. The built bundle under bun.

Desktop-exported `GLM_BINARY_PATH` and `ZCODE_AGENT_SERVER_COMMAND` are deliberately not launcher overrides. Children use `env_clear()` and an explicit allowlist, including resolved roots, proxies, supported provider/ripgrep overrides, and custom CA variables.

### Bun source setup

The branch's root `package.json` currently declares `bun@1.4.3` (upstream v3.15.1 still declares pnpm). `vitest` is pinned through `overrides` so upstream's vitest patch applies under Bun. Source mode uses the runtime portability change that probes `node:sea` instead of statically importing it.

```sh
bun install --frozen-lockfile --ignore-scripts
# Add --backend=copyfile where ReFS hard links are unavailable.
for p in contracts dynamic-workflow adapters shared-types core dynamic-workflow-runtime i18n telemetry bootstrap tui; do
  (cd apps/zcode-cli/packages/$p && bun run --bun build)
done
```

`contracts` reports TS2589 under Bun's hoisted layout (its `zod@3` vs the hoisted `zod-to-json-schema` peer on root `zod@4`); `tsc` still emits `dist` and runtime is unaffected. This predates v3.15.1. Installer-only bundled plugins may be absent in source mode. The upstream CLI single-file build script still relies on `tsx`; GPUI does not use it.

### Compiled backend (prototype)

```sh
bun apps/zcode-gpui/scripts/build_backend.ts            # -> apps/zcode-gpui/target/backend/
bun run package:gpui -- --with-backend                  # stages it into dist/zcode-gpui/
ZCODE_GPUI_SMOKE_BACKEND=<path>/zcode-backend.exe bun apps/zcode-gpui/scripts/backend_smoke.ts
```

`bun build --compile` turns the unchanged CLI entry into one executable (about 151 MiB, built in about 4 s). Three Bun-runtime-path backend edits make it work: the workflow and plugin-host self re-exec use hidden subcommands, as under Node SEA, because the entry is a virtual `B:/~BUN/root/…` path; the provider release is located beside `process.execPath`; and the ripgrep worker imports the staged `node_modules/ripgrep` by file URL, because compiled workers cannot resolve bare package names. Without that staged package, text search silently falls back to the JavaScript engine. Measured with GPUI on an isolated profile at idle: 176 MiB working set and 277 MiB private, versus 251 MiB and 872 MiB for bun running the source.

Known gaps: official plugins are not staged. The compiled binary finds them only through a repository checkout in its working directory chain, and its plugin-root lookup still uses the virtual entry directory. TUI, browser automation (playwright-core) and koffi stay external, as in the upstream CLI build. There is no signing or installer.

### Fast advisory type check

`bun run typecheck:fast` runs `bun check -b` (Bun 1.4.3, TypeScript 7 semantics) over the root `typecheck` projects (`packages/shared`, `packages/browser-commands`, `apps/zcode-browser-extension`) in about 2 s. It is advisory; `bun run typecheck` (tsc) remains the gate. The CLI has its own `bun run typecheck` in `apps/zcode-cli`.

### Browser Use setup

```sh
cargo build --release --manifest-path apps/zcode-gpui/Cargo.toml   # also builds zcode-browser-host
bun apps/zcode-browser-extension/scripts/build.ts                   # -> apps/zcode-browser-extension/dist/
bun run package:gpui                                                # stages host + browser-extension/ into dist/zcode-gpui/
```

Load the extension unpacked from `chrome://extensions` (Developer mode on), then click Register in Settings › Browser Use. GPUI finds the extension in `browser-extension/` next to the executable, or in the checkout's `apps/zcode-browser-extension/dist`. Pass `--skip-extension` to the packager to package without bun.

### Voice setup

```sh
# Needs cmake and libclang (LLVM, or `pip install libclang` and point LIBCLANG_PATH at its `clang/native` dir).
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release --manifest-path apps/zcode-voice/Cargo.toml
bun run package:gpui                  # stages zcode-voice.exe, DirectML.dll and espeak-ng-data/ into dist/zcode-gpui/
```

GPUI finds `zcode-voice` through `ZCODE_GPUI_VOICE_PROGRAM`, next to its executable, or in the checkout's `apps/zcode-voice/target/{release,debug}`. A dev build reads eSpeak NG data from its own cargo build output, so no system eSpeak install is needed. Pass `--skip-voice` to the packager to package without it; a failed voice build only warns. Packaging currently stages voice on Windows only. See [apps/zcode-voice/README.md](../zcode-voice/README.md).

## Validation

```sh
cargo fmt --manifest-path apps/zcode-gpui/Cargo.toml --check
cargo clippy --manifest-path apps/zcode-gpui/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/zcode-gpui/Cargo.toml
bun apps/zcode-gpui/scripts/backend_smoke.ts
cargo test --manifest-path apps/zcode-voice/Cargo.toml   # needs the voice build environment above
# Browser command layer and extension (vitest needs Node, not the Bun runtime):
node node_modules/vitest/vitest.mjs run packages/browser-commands apps/zcode-browser-extension
```

The backend smoke needs built CLI dependencies. It creates isolated temporary roots and a loopback mock API-key provider, checks CLI-owned draft/session slash catalogs, disabled custom filtering and actual `/init` prompt expansion, chunked image upload/idempotence/abort and artifact resolution into a vision-capable model request, one-shot idle startNow admission, deterministic busy queue admission (including frozen plan/mode intent) and held-queue missing/stale refusal plus Keep/Clear results, strict runtime-preference requests in both scopes with Memory enabled, actual assistant frames in both delivery modes, a new completed recovery snapshot, source lifecycle and plugin Describe, scoped typed config persistence, secret omission/preservation/explicit clear, both reset semantics, saved-run completion/history, published markdown-artifact metadata/content, cross-parent content-read refusal and unknown-run empty results, metadata replacement, Move/refusal and Delete of its own disposable fixtures, then cleans up. It does not use real credentials or production session data. It is a CLI/protocol smoke, **not a full GPUI-executable-to-rendered-transcript test**.

GPUI test windows exercise Settings keyboard/QuickPick entry, measured-bound pointer clicks, shortcut recording, draft/focus preservation, preference failures/ordering, and streamed state while Settings is open. Native platform rendering/visual acceptance and an actual executable restart remain unverified. Current results and remaining gaps are recorded in `SETTINGS.md`.

## Troubleshooting

- `no backend`: Check that the directory exists and a launch candidate has built dependencies.
- `model_request_failed`: Add or enable an API-key provider in Settings › Model settings and select one of its models ([PROVIDER_SETTINGS.md](PROVIDER_SETTINGS.md)).
- Preference refusal while Electron is running: Change settings there, or close it before using GPUI's preference writer.
- The browser tool lists no browsers: check Settings › Browser Use. The bridge must be listening (only one GPUI instance owns it; scratch instances never start it), the host must be registered for this copy, and the extension must be loaded and enabled. It reconnects within a minute, or right away after you reload it.
- Settings › Voice says "Voice engine not found": build `apps/zcode-voice` (see Voice setup) or put `zcode-voice.exe` next to GPUI. An eSpeak error means `espeak-ng-data` is missing next to it. The agent only gets the `Speak` tool in sessions started or resumed after turning voice on.
- `ZCODE_GPUI_LOG_STDOUT=1`: Emits frontend diagnostic logs to stderr. Diagnostics must remain redacted.

## License

zcode-gpui is Apache-2.0, like the rest of this repository; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for its dependencies. The optional voice program `apps/zcode-voice` is a separate GPL-3.0-or-later executable because it statically links eSpeak NG; GPUI only starts it and talks to it over stdin/stdout.
