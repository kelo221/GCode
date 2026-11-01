# GCode: a ZCode fork with a native GPUI client and agent improvements

> **Unofficial.** GCode is a personal fork of [zai-org/ZCode](https://github.com/zai-org/ZCode). It is not an official ZCode product and is not supported by the ZCode team.

This README only covers what the fork adds. For ZCode itself (the Electron desktop app, the Web client, the CLI distribution, packaging, configuration and the project notice), see the [upstream repository](https://github.com/zai-org/ZCode) and its README.

The fork tracks upstream releases (currently v3.16.2) and keeps its changes on `main`. The Electron desktop app and the Web client are left as upstream ships them.

## What the fork adds

### A native desktop client: GCode (`apps/zcode-gpui`)

GCode is a Rust frontend built with GPUI (Zed's UI framework) that talks to the unchanged ZCode CLI app-server over stdio. It does not need Electron, so it uses about 300 MB less memory than the Electron desktop app and less CPU. See [apps/zcode-gpui/README.md](apps/zcode-gpui/README.md) for setup, features and current gaps compared with the desktop app.

Highlights beyond a plain port:

- **Noise-free transcript.** A composer toggle (on by default) folds runs of tool calls into one line and keeps agent text, todo updates, questions, plans and thinking visible. Thinking is shown open in a muted style.
- **Browser Use in your own browser.** The agent's browser tool drives tabs in Helium or Google Chrome through a ZCode extension ([apps/zcode-browser-extension](apps/zcode-browser-extension/README.md)) and a small native host. See [BROWSER_BRIDGE.md](apps/zcode-gpui/BROWSER_BRIDGE.md).
- **Voice.** Optionally, the main agent gets a `Speak` tool and says a short sentence on your PC when it needs you or finishes. Speech is generated locally by [apps/zcode-voice](apps/zcode-voice/README.md) (Kokoro). See [VOICE.md](apps/zcode-gpui/VOICE.md).
- **Phone remote control.** An Android build of the same window ([apps/zcode-gpui-android](apps/zcode-gpui-android/README.md)) pairs with the desktop over the local network by QR code. See [REMOTE_CONTROL.md](apps/zcode-gpui/REMOTE_CONTROL.md).
- **System prompt override.** Settings › System prompt replaces the main agent's system prompt for every workspace.
- **Secrets vault instead of `.env` files.** Settings › Secrets stores tokens in the OS secret store (Windows Credential Manager, macOS Keychain, Linux Secret Service) and keeps only their names on disk. They reach the agent's Bash commands and stdio MCP servers as environment variables, for every workspace or only the ones you pick, and survive rebuilds. Any secret value in tool output is replaced with `***` before the model or the transcript sees it. See [SECRETS.md](apps/zcode-gpui/SECRETS.md).
- **Agent branches review.** The Review pane lists branches produced by isolated subagents (see below) and shows their diff against the commit they started from, read-only.
- **Idle unload that respects background work.** Inactive workspaces unload their agent after 10 minutes, but not while background subagents, shells or workflows are still running.
- **Provider management.** Settings › Model settings has the desktop "Model Provider" section for API-key providers. You can add, edit, enable and delete providers, add, reorder and test their models, and start from a template. Edits go through the CLI. See [PROVIDER_SETTINGS.md](apps/zcode-gpui/PROVIDER_SETTINGS.md).
- **Running agents in the Review pane.** Each running subagent card shows its model, elapsed time, current todo and progress. The Git file list sits behind a one-line branch and change summary.
- **Mention chips.** Task (`#`), skill (`$`), file, folder and plugin references show as chips in the composer and in sent messages, as in Electron. The text sent to the agent is unchanged.
- **Fast `@` file search for large projects.** A per-workspace file index is built in the background and kept up to date by a file watcher. It honours `.gitignore`, `.ignore` and `.zcodeignore`, has no depth or file-count limit, and skips Unity and build folders and `*.meta` files. Results are ranked fzf-style: word starts, camelCase humps and consecutive letters score highest, so `pmgr` finds `PlayerManager.cs`. Electron's ranking is simpler.
- **Sidebar ready at startup.** The newest tasks of every project and of Tasks are cached and shown immediately. The Tasks agent and the agents of folders you left open start in the background and preload their 3 newest tasks. Projects are ordered by their latest task activity, and a running task doesn't make projects swap places.
- **Text selection and editing.** A whole Markdown reply can be selected across paragraphs, and right-click offers Copy as Markdown. The composer has undo/redo, Up-arrow recall and visual-line navigation. Shortcuts hide the sidebar (Ctrl+B), zoom (Ctrl+= / - / 0) and go to the previous or next task (Ctrl+Shift+[ / ]).
- **Local images in replies.** `![alt](path)` images that point at absolute local paths or `file://` URLs show as thumbnails under the assistant's reply.
- **Unfinished subagent work.** When a subagent stops before finishing, its `<remaining_items>` block shows as an "Unfinished items" list instead of raw tags.
- **Separate GPUI settings.** GPUI-only preferences live in `~/.zcode/v2/gpui_settings.json`, so they can be changed while the Electron app is running and the desktop never drops them.

### Agent and CLI changes: `apps/zcode-cli`

GPUI turns these on through optional runtime preferences. The Electron app never sends them, so its behavior is unchanged.

- **Built-in code search.** `FindFiles` (fuzzy path search) and `SearchCode` (plain or regex content search) use an in-process fff index per workspace. The index closes about a minute after the workspace goes idle. Setting: Agent capabilities › Code search.
- **Diagnostics after edits.** After Edit, Write or ApplyPatch, the agent receives errors and warnings for that file from its language server (rust-analyzer, csharp-ls or typescript-language-server, whichever is on `PATH`). A `Diagnostics` tool runs a full project check. Results state whether they are confirmed, best effort or stale; silence is never reported as a clean file. Spec: [lsp-diagnostics.md](apps/zcode-cli/docs/design/v2/tool/lsp-diagnostics.md).
- **Worktree isolation for subagents.** `Agent` accepts `isolation: "worktree"`, or an agent file can set `isolation: worktree`. The subagent then works in its own git worktree under `.zcode/worktrees/` on branch `zcode/<agentId>`, starting from HEAD (your uncommitted changes are not included). When it finishes, its changes are committed to that branch, or the worktree is removed if nothing changed, and the parent agent is told how to merge it. In a jj repo (and with `jj` on `PATH`) it gets a jj workspace and bookmark `zcode/<agentId>` instead. Spec: [07-subagent.md](apps/zcode-cli/docs/design/v2/tool/07-subagent.md).
- **Subagent IDs survive restarts.** `SendMessage` can resume a subagent from its saved metadata after the CLI restarts, a foreground subagent fails, or the conversation is compacted. A `ListAgents` tool lists a session's subagents.
- **Agents use the index for searching.** When code search is on, the agent, Explore subagents and custom agents that list Grep/Glob are pointed at `FindFiles`/`SearchCode` instead of `grep`/`find` through Bash.
- **A closing `Speak` ends the turn.** A reply that comes with only `Speak` calls no longer triggers another model request.
- **In-memory secrets for tools.** A `secrets/sync` request hands the agent the secrets from GPUI's vault. It never writes them to disk or logs them. It adds them to Bash and stdio MCP environments and masks their values in tool results, tool errors and live tool output.

These apply to every client:

- **`Compact` tool.** The main agent and subagents can compact their own context when you (or the parent agent) ask them to. `SendMessage` also works in plan mode, and a resumed agent still can't write.
- **"Send now" keeps subagents running.** Sending a message mid-turn used to cancel foreground subagents. They now move to the background, report back when done, and the next turn is told not to start them again. Stop still cancels them.
- **Subagents keep working after auto-compaction.** Subagents used to quit soon after compacting, claiming their "budget is spent". Their compaction summary now keeps the original brief and a checklist of unfinished work. They end unfinished work with a `<remaining_items>` list, and the parent is told to continue them with `SendMessage`. Usage reports say plainly that the token figures are not a budget.
- **`ReadSessionContext` runs on the Explore model**, when one is configured, instead of the turn's main model.
- **Sessions no longer break permanently on two provider errors.** Empty assistant turns are no longer replayed to Anthropic (a 400 "text content blocks must be non-empty"). Pasted BMP/ICO/TIFF images are converted to PNG, and every image part sent to a model declares a media type the provider accepts. Sessions already hit by either error recover.
- **Telemetry is off.** The CLI's OpenTelemetry export is hard-disabled and cannot be turned back on by environment variables. GPUI sends no telemetry of its own.

### Tooling

- **Bun instead of pnpm and Node.** The root `package.json` declares `bun@1.4.3`. `bun run typecheck:fast` is an advisory type check with Bun's TypeScript 7 checker; `bun run typecheck` (tsc) stays the gate.
- **Root scripts are GPUI-only.** The root `package.json` keeps only the scripts GPUI and its bundled CLI use: run/package GPUI, CUA build, typecheck/lint/format, architecture check and `dep:refs`. The Electron, web, E2E and release scripts were removed. `bun run verify:pre-push` (the pre-push hook) runs `cargo fmt --check` and clippy on GPUI.
- **Compiled backend (prototype).** `apps/zcode-gpui/scripts/build_backend.ts` compiles the CLI app-server into a single executable with `bun build --compile`.
- **`packages/browser-commands`.** Browser commands moved out of the Electron package so the extension and the desktop app share them.

## Download (Windows)

Windows x64 builds are on the [Releases page](https://github.com/kelo221/ZCode/releases). Unzip anywhere and run `zcode-gpui.exe` (GCode). The zip includes the compiled CLI backend, the browser extension and native host, and the voice program, so no checkout, Node or Bun is needed. The executables are not code-signed.

Releases are built by [gpui-release.yml](.github/workflows/gpui-release.yml). Pushing a tag such as `gpui-v3.16.2-1` (upstream version, then the fork's build number) publishes one; running the workflow by hand only produces a downloadable artifact.

## Building from source

Requirements: Git, [Bun](https://bun.sh/) 1.4.3 and a Rust toolchain (edition 2024).

```sh
bun install --frozen-lockfile --ignore-scripts
for p in contracts dynamic-workflow adapters shared-types core dynamic-workflow-runtime i18n telemetry bootstrap tui; do
  (cd apps/zcode-cli/packages/$p && bun run --bun build)
done
bun run start:gpui    # opens the repository itself as the workspace
```

To open another project, run `cargo run --release --manifest-path apps/zcode-gpui/Cargo.toml -- --workspace <absolute_project_path>`. `bun run package:gpui` stages a runnable copy in `dist/zcode-gpui/`. The optional browser extension, native host and voice program have their own build steps in [apps/zcode-gpui/README.md](apps/zcode-gpui/README.md).

Models are reached through API-key providers. Add one in Settings › Model settings. Account sign-in (Zhipu account) providers are not supported.

## Documentation

| Topic                                           | Where                                                      |
| ----------------------------------------------- | ---------------------------------------------------------- |
| GPUI client setup, features and troubleshooting | [apps/zcode-gpui/README.md](apps/zcode-gpui/README.md)     |
| What GPUI covers compared with the desktop app  | [apps/zcode-gpui/PARITY.md](apps/zcode-gpui/PARITY.md)     |
| GPUI settings                                   | [apps/zcode-gpui/SETTINGS.md](apps/zcode-gpui/SETTINGS.md) |
| Everything else about ZCode                     | [zai-org/ZCode](https://github.com/zai-org/ZCode)          |

## Staying in sync with upstream

Upstream is the `upstream` remote and is merged into `main` on each release:

```sh
git fetch upstream
git merge upstream/main
```

Upstream edits to `README.md` and `README.en.md` will conflict with this file. Keep this README and keep `README.en.md` deleted.

## License

Apache-2.0, like upstream; see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). The optional voice program `apps/zcode-voice` is a separate GPL-3.0-or-later executable because it statically links eSpeak NG. GPUI only starts it and talks to it over stdin/stdout.
