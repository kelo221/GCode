# GPUI Frontend Parity Plan

Status: **action-level audit updated 2026-10-06**. The first Settings/safety slice is
implemented; the broader parity backlog is not complete. This file is the domain spec
for `apps/zcode-gpui`: update it before changing behavior. Current status and evidence
limits below take precedence over the retained historical milestone requirements.

## Plan overlay, long text in queue and composer (2026-10-21)

- **Queued rows are one line.** Each queued row shows its text collapsed to a single line
  (newlines become spaces), capped at 200 characters and ellipsized at the row width
  (Desktop `ConversationQueuePanel.tsx` `truncate`). The row list is capped at 200px and
  scrolls with the wheel. Edit still loads the full text into the composer.
- **Composer caps its height.** The input grows up to 160px (Desktop `LexicalChatInput.tsx`
  `max-h-40 overflow-y-auto`), then scrolls with the wheel. Typing, pasting and caret keys keep
  the caret visible. Pastes of 15,360 UTF-16 units or more still become a text attachment.
- **Plan is an overlay, not a mode.** The CLI keeps `config.mode` as the permission mode
  (`build | guarded | yolo`, legacy `edit`) and toggles `config.planEnabled` separately
  (`packages/shared/src/execution-state.ts`). GPUI mirrors both. The mode trigger always shows the
  permission label; when plan is on, a "Plan" chip next to it turns plan off on click. The menu is
  Desktop's (`V4ComposerModeControls.tsx`): a "Plan mode" check row, a separator, then the
  permission radios Ask before changes / Autonomous mode / Full access. Edit automatically is
  listed only while it is the current mode.
- **Commands.** In a live session, plan on sends `switchCollaborationMode{mode:"plan"}` and plan
  off sends `switchCollaborationMode{mode:<current permission>}`. A permission pick while plan is
  off sends `{mode:X}`. A permission pick while plan is on only updates the draft submission
  override `{mode:X, planEnabled:true}`, which rides on the next send, because `{mode:X}` alone
  would also clear plan. New-task drafts submit the chosen (or workspace default) permission mode
  plus `planEnabled`, never a forced `build`.
- **Leaving plan keeps the permission mode.** ExitPlanMode approval only clears `planEnabled`, so
  a session in Full access + Plan returns to Full access. Approval never escalates a session to
  Full access by itself.
- **Agent-driven transitions win.** When `config.planTransition.toolCallId` changes
  (EnterPlanMode/ExitPlanMode), any `mode`/`planEnabled` left in that session's draft override is
  dropped (Desktop `useDraftConfigControl.ts` planTransition rebase).
- Not ported: menu row descriptions, and the warning colour on the Full access trigger (the
  vendored ely menu host has neither).

## Prompt cache lifetime (GPUI-only, 2026-11-03)

- **What already happens.** The CLI marks the system prompt blocks and the newest
  conversation message with Anthropic `cache_control: {type: "ephemeral"}` on every
  request (`core/src/context/builder.ts`, `runtime/helpers/provider-request-messages.ts`),
  which means the default 5-minute cache. This setting only changes those markers.
- **Setting (per provider).** Each provider card in Settings › Models whose API format is
  Anthropic Messages has a "Prompt cache" row: 5 minutes (default), 1 hour, Off. Other
  formats hide the row (the AI SDK never sends Anthropic cache settings to them). One
  provider can be the real Anthropic API while another is a proxy that rejects `ttl` or
  `cache_control`, so the value is per provider ID. It is stored as
  `gpuiPromptCacheByProvider: {providerId: mode}` in `gpui_settings.json` (Electron drops
  unknown keys from `setting.json`), so it saves while the desktop runs. A missing entry or
  unknown value means 5 minutes; choosing 5 minutes removes the entry. Desktop only; the
  phone has no agents of its own. (The first, global version used `gpuiPromptCache`; that
  key is ignored.)
- **Runtime.** GPUI answers `session/requestRuntimePreferences` with `promptCacheModes`
  (`{[providerId]: "5m" | "1h" | "off"}`, optional; Electron never sends it, which means
  `5m` everywhere). The CLI reads it when a session is created or cold-resumed and hands the
  map to that session's model adapter. Running sessions keep their map. In-process
  subagents and workflow children share the parent's adapter, and protocol child sessions
  inherit the parent's map.
- **Wire.** Applied once per logical request in the adapter's Anthropic history projection
  (`adapters/src/model/runner.ts` `projectRequestHistory`), using the entry for the
  request's provider ID, so every marker in a request gets the same lifetime (Anthropic
  requires longer TTLs to come before shorter ones).
  `5m` leaves the markers untouched, so the request bytes match the old behavior. `1h`
  sets `ttl: "1h"` on every marker (writes cost 2× input instead of 1.25×; reads 0.1×
  either way). `Off` removes every marker, for proxies that reject `cache_control`.
  Non-Anthropic providers are unaffected.
- **Acceptance.** Adapter tests check the sent body for each mode and that one provider's
  entry never applies to another; the GPUI reverse-RPC test checks the map; a render test
  checks the row shows only for Anthropic Messages providers; a live check reads `cache_creation` in the model-io log after a
  1-hour turn.

## Load MCP tools on demand (ToolSearch setting, 2026-10-30)

- **Setting.** Settings › MCP Servers has a "Load MCP tools on demand" select with Auto, Local and
  Off. It writes the shared `setting.json` key `toolSearchMode` (Desktop TS11,
  `apps/zcode-cli/docs/design/v2/tool/19-tool-search.md`), so saving is refused while the
  desktop runs. A missing or unknown value means Off.
- **Runtime.** GPUI answers `session/requestRuntimePreferences` with `toolSearchMode`. The CLI reads
  it when a session is created or cold-resumed. Running sessions keep their mode, and subagents
  inherit the parent's. With Auto or Local, `mcp__*` schemas are deferred: the model sees their
  names and loads a schema with `ToolSearch` (`select:<name>`) when it needs one.
- **Transcript.** A `ToolSearch` row's summary lists the tool names for a `select:` query, and
  otherwise shows the quoted search query.

## New-task draft model and thinking level (2026-10-30)

- **Each new-task draft keeps its own picks.** The model, thinking level, permission mode and plan
  toggle chosen in a `draft:<workspace>` composer are saved with the draft text when you navigate
  away, and restored when you come back. Existing threads never inherit them.
- **New drafts start from the last used model.** After a create or send is accepted (never
  optimistically), GPUI writes the model the message used, `gpuiLastComposerModel {provider, model,
  thought}`, to `gpui_settings.json`. A fresh draft uses it when this workspace's catalog offers that model, and
  uses the thinking level only when the model offers it. Otherwise it falls back to the model's
  default thinking level and the workspace default model (Desktop `composerRecent.ts`, kept global
  rather than per scope).

## Transcript file references (GPUI-only, 2026-10-30)

- **What gets linked.** Inline code in assistant Markdown becomes a link when it is a single token
  that looks like a file path: it contains `/` or `\`, or ends in a short extension (`PLAN.md`,
  `src/a.rs:42`, `C:\x\y.ts`, `~/notes.md`), with an optional `:line[:col]` or `#L12` suffix.
  Commands, flags, globs, URLs and version numbers are not linked. Since 2026-10-31 such inline
  code is linked only when the file exists, by the same resolution rule as a click below, so a
  symbol like `ExplosionUtils.Explode` stays plain code. Results are cached (found 60 s, missing
  5 s); a bare name stays linked until the workspace `@` index has been built. Markdown links without a URL
  scheme (relative, absolute, `file://`) use the same path. Desktop links neither, so this is a
  GPUI extension.
- **Resolution, at click time.** Paths resolve against the active workspace. `..` cannot escape it
  for relative paths. A bare name that is not at the workspace root is looked up by file name in
  the workspace and opens only if exactly one file matches.
- **What a click does.** A file opens with the system default app (since 2026-10-31; it used to
  open workspace files in the Files pane preview). A file with a line number opens in the external
  editor at that line (`code -g`, falling back to the default app). A missing file shows a notice
  and opens nothing.

## Answering agent questions in the card (2026-10-21)

- **The card replaces the composer.** While a blocking interaction (any `permission` or
  `userInput`, not `workspaceHookReview`) is pending, its card sits in the composer's slot,
  under the queue panel, and the composer is hidden. The composer entity keeps its draft, and
  Enter can no longer queue a normal message that leaves the question unanswered (Desktop
  `ConversationComposer.tsx` `display:none` while `blockingRequestId` is set). Only the first
  blocking interaction shows (Desktop `V4InteractionDialogs.tsx`); workspace hook reviews still
  show alongside it.
- **Typed answers live in the card.** One single-line input sits in the card, under the options:
  - Questions: placeholder "Enter your answer...", one draft per question that survives
    Back/Continue. The typed text is sent along with the picked options, joined into
    `answers[question]` and `answer_<i>` (an array for multi-select; `answer` too when there is
    only one question). For single-select questions, typed text replaces the picked option,
    and picking an option clears the typed text (Desktop `ElicitationDialog.tsx`
    `getQuestionAnswers` / `updateElicitationDraftsForOption`). Enter goes to the next
    question, or submits on the last one.
  - Plan approval: the input is the change request; "Request changes" or Enter sends it as
    feedback.
  - Permission with `freeText`: placeholder "Tell the model what to do instead...";
    "Deny with feedback" or Enter denies with that text (Desktop `PermissionDialog.tsx`).
  - Questionless `userInput` with `freeText`: "Send answer" or Enter sends the text.
- On the desktop the input takes focus when the card appears, and focus returns to the
  composer once the card is answered. The phone never focuses it automatically (soft keyboard
  rule).
- Empty-text Enter on plan approval and permission cards does nothing; Approve and Deny stay
  explicit buttons.

## Diagnostics after edits (2026-10-20, fork CLI + GPUI)

- No Electron counterpart. Settings › Agent capabilities › Diagnostics, switch "Diagnostics after
  edits" (`gpuiLspDiagnostics` in `gpui_settings.json`, absent = on; desktop only, hidden on the phone).
  GPUI answers `session/requestRuntimePreferences` with `lspDiagnosticsEnabled`; it applies to
  new and resumed sessions. CLI spec: `apps/zcode-cli/docs/design/v2/tool/lsp-diagnostics.md`.
- After an `Edit`/`Write`, the CLI asks the file's language server (rust-analyzer, csharp-ls,
  typescript-language-server, when on PATH) for that file's errors and warnings and appends them
  to the model's tool result. A `Diagnostics` tool runs a full check (Rust: `cargo check` through
  rust-analyzer).
- Edit/Write cards show a chip next to the summary from the `file_diff` display's `diagnostics`
  summary: red "N errors", yellow "N warnings", muted "No problems", or muted "Diagnostics
  pending" while the server starts or all server slots are busy. A "?" suffix marks results that
  are not confirmed fresh or that timed out. No chip when the language has no server installed.
- In noise-free mode the fold line adds "· N errors": the latest edit result per file inside that
  run, so an error fixed by a later edit in the same run is not counted.

## Agent branches (read-only, 2026-10-20, fork GPUI)

- No Electron counterpart. The Review pane lists "Agent branches" above the working-tree header:
  `git worktree list --porcelain` entries under `<repo>/.zcode/worktrees/` on `zcode/agent_*`
  branches, each with "N files +A −D" from `git diff --shortstat <base> <head>`. The base is
  `git config branch.<branch>.zcodeBase` (written by the CLI), else `git merge-base HEAD <branch>`.
  The list refreshes with the pane and is hidden when empty.
- Clicking a branch switches the pane to compare mode, pinned to that worktree's HEAD SHA at the
  time of the click: files come from `git diff --name-status`/`--numstat <base> <head>`, and an
  expanded row shows `git diff <base> <head> -- <path>`. Refresh re-reads the same pinned pair.
  The header shows the branch and both short SHAs with a "Working tree" button to go back.
  Uncommitted changes inside a still-running agent's worktree are not shown.
- Compare mode is read-only: Stage all, the Unstaged/Staged tabs, per-file stage/discard/unstage,
  the branch switcher, Commit and Push are hidden, and git actions are refused if reached anyway.
  Merging stays a model or terminal step (`git merge --no-ff zcode/<id>`); Merge/Discard buttons
  are a later round.
- The diff cache key includes the target (working tree or `base..head`), and switching target
  bumps the pane generation, so a diff that was loading for the previous target is dropped.
  Switching workspace returns to the working tree.
- Failed git commands now report the exit code and stderr, or stdout when stderr is empty (hooks
  often print to stdout), instead of an empty notice.
- jj repos (2026-10-23): when the repo root has `.jj` and `jj` runs, the list also includes
  `zcode-agent_*` workspaces from `jj workspace list --ignore-working-copy`. The head is the
  `zcode/<id>` bookmark, else the workspace's `@-` (uncommitted work is not shown, as with git).
  The base is `jj config get zcode.base.<id>`, else `git merge-base`. Diffs still run through git,
  so only colocated jj repos are listed.

## Agent worktree isolation (2026-10-20, fork CLI)

- CLI side of "Agent branches" above. Spec: `apps/zcode-cli/docs/design/v2/tool/07-subagent.md`
  ("Worktree isolation 当前范围"). `Agent` takes `isolation: "worktree"`, and an agent file can set
  `isolation: worktree` as its default. Only local git workspaces get it; elsewhere the call fails
  before launch, and the agent never runs in the parent checkout instead.
- The agent runs in `<repo>/.zcode/worktrees/<agentId>` on branch `zcode/<agentId>`, starting from
  HEAD. The parent's uncommitted changes and untracked files are not included. `.zcode/worktrees/` is
  added to the repository's exclude file. Bash, Read/Write, FindFiles/SearchCode (separate index) and
  Diagnostics all resolve in the worktree. Cargo builds share the main checkout's `target` unless
  `CARGO_TARGET_DIR` is already set.
- When the agent really finishes (not when it is moved to the background), background commands it
  left running are stopped, its changes are committed ("zcode agent <id>: <description>"), and the
  result ends with a `<worktree_result>` block: branch, diff stat and `git merge --no-ff zcode/<id>`.
  No changes means the worktree and branch are removed. A failed commit (hooks, signing) keeps
  everything and reports the error next to the agent's own result.
- Resuming with `SendMessage` reuses the worktree, recreating it from the branch if the directory
  is gone, or fails with "isolation unavailable". `ListAgents` shows the branch and path.
- jj repos (2026-10-23): when `jj root` succeeds and `jj` is on PATH, the agent gets a jj workspace
  `zcode-<agentId>` at the same path instead, based on the parent workspace's `@-`. On finish its
  working copy is described ("zcode agent <id>: <description>") and bookmark `zcode/<agentId>` is
  set; no changes means the workspace is forgotten and removed. The result tells the parent to
  integrate with `jj rebase -b zcode/<id> -d @- && jj rebase -s @ -d zcode/<id>`. Git commands
  inside a non-colocated workspace are blocked with `GIT_CEILING_DIRECTORIES`.

## Subagent IDs survive restarts and failures (2026-10-20, fork CLI + GPUI)

- The CLI spec is `apps/zcode-cli/docs/design/v2/tool/07-subagent.md` ("SendMessage 当前范围").
  `SendMessage(to: agent_…)` falls back to `~/.zcode/cli/agents/<session>/<agentId>/metadata.json`
  when the in-memory registry has lost the agent (CLI restart, idle unload, crash). It then resumes
  the same child conversation. A metadata status of `running` means the owning process died, so it
  is treated as `stopped`. Only the current parent session's agents resolve.
- When a foreground Agent fails, is cancelled or hits the inactivity watchdog, the parent model's
  tool result now ends with `agentId: … SendMessage …`. The UI error text stays the root cause.
- The new deferred `ListAgents` tool lists this session's subagents (live and on-disk) with their
  IDs. After a compaction, a reminder lists the 20 most recent ones.
- GPUI: subagent cards in the transcript and in the dock's Agents section show a mono chip with
  the short agent ID (`agent_7fbbd55f`). Clicking it copies the full ID, so the user can say
  "resume agent_…". Electron has no counterpart. The ID comes from the dock's `agentId` when
  present, else from the child session name `sess_subagent_<agentId>`.

## Review dock: running agents first, git behind a summary (2026-10-22, fork CLI + GPUI)

- Protocol (`packages/shared/src/zcode-protocol-v4/snapshot.ts`): each `subagents.running[]` item
  may carry `model` (`provider/model` the child actually runs on) and `todos` (the child's latest
  TodoWrite list, same shape as `plan.items`). Both are optional; older CLIs omit them.
- CLI: the child runtime reports its resolved model through `onSessionReady`, so `SubagentSpawned`
  carries `model` even when the agent inherits the parent's model (before, only a profile-pinned
  model was sent). The parent projection records each child's mirrored TodoWrite
  (`source: "subagent"`, keyed by `childSessionId`) and emits it with the running item; it never
  enters the parent's plan or timeline, and is dropped when the child stops. Todos are live only:
  after a cold reload they reappear with the child's next TodoWrite. The model survives because
  `SubagentSpawned` is persisted.
- GPUI dock order: Progress (the main session's plan), Goal, Agents, background activity, agent
  branches; this area scrolls and takes the whole dock while the git list is closed. Each running
  agent card (`review/agent_card.rs`) shows the title, a model chip (catalog name when the
  workspace knows the model, else the model id), elapsed time, the ID chip, Stop / Open, then the
  current todo (in progress, else next pending; "All todos done" once complete) with a
  `completed/total` counter and a thin progress bar, and a one-line preview of the summary.
- Ended agents show one line of their report (first line with text, markdown markers stripped)
  instead of the whole report.
- Git (`review/git_summary.rs`): one summary row with the branch chip (click cycles branches), the
  file count, +/- totals and Refresh. Clicking the row opens the Unstaged/Staged tabs, Stage all,
  the file list and the Commit/Push bar, which then take the lower half; it starts closed and the
  choice is kept per window (`GitState.list_open`). An agent-branch comparison is always open.
- Electron's side panel still lists files directly; this layout is GPUI-only.

## Idle agent unload keeps background work alive (2026-10-20, GPUI-only)

- No Electron counterpart (Electron never unloads an agent). Every 30 s GPUI unloads (kills)
  the CLI agent of a workspace that is not the active one, has no open phone view, no pending
  startup/browser command, nothing busy, and no user activity for `ZCODE_GPUI_IDLE_SECS`
  (default 600 s).
- "Busy" means any of the workspace's sessions has: an active `phase` (`running`,
  `prewarming`), `hasBackgroundWork: true` in the sessions index, or, in an open
  conversation, a running phase, a pending interaction, or running `backgroundWorks` /
  subagents. Background subagents, shells and workflows live only in that CLI process's
  in-memory task registry, so killing it ends them mid-task: `TaskOutput` can't read them back,
  and `SendMessage` can only restart them from their saved conversation (see "Subagent IDs
  survive restarts and failures") (bug seen 2026-10-19: three background Explore agents were
  killed 600 s after the user's last command, once the parent turn had ended and the user had
  switched to another project).
- Owner of the facts: the CLI. `hasBackgroundWork` is derived from the session snapshot's
  running `backgroundWorks`; GPUI only reads it. Unloads are only recorded in the in-memory
  GPUI log (`push_log`), so after the fact an unload shows in the CLI log as the process
  stopping mid-stream with no shutdown lines.

## Built-in fff code search (2026-10-19, GPUI-only)

- No Electron counterpart. Sessions started by GPUI get two built-in, read-only tools that
  need no permission prompt: `FindFiles` (fuzzy file-name search) and `SearchCode` (content
  search: plain text, Rust regex when the query has regex syntax, a fuzzy retry when nothing
  matches, `patterns` for multi-pattern OR, `files_with_matches`/`count` modes, context
  lines and paged `cursor`s). Both use fff's constraint tokens (`src/`, `*.ts`, `!test`) and
  return paths relative to the workspace root. They replace the user-configured `fff` MCP
  server, which had one fixed root for every project.
- Gate: GPUI answers `session/requestRuntimePreferences` with `codeSearchEnabled`
  (Settings › Code search, stored as `gpuiCodeSearch` in `gpui_settings.json`, absent = on; desktop
  only, hidden on the phone). The CLI injects a `CodeSearchPort` only when that is `true` and
  the fff native library loads, so Electron (which never sends the field) and any failed load
  leave the tools unregistered and the agent searches with Bash. Subagents and workflow child
  agents share their parent's port; the Explore subagent may use both tools.
- Steering (2026-10-22): when both tools are registered for an agent, its prompts point at them
  instead of shell search. The Bash description lists `find`, `grep` and `rg` among the commands
  to avoid and names FindFiles/SearchCode as the replacement; the Explore system prompt tells it
  to search with FindFiles/SearchCode and keep Bash for other read-only commands. A custom agent
  profile whose `tools:` list names `Glob` or `Grep` also gets `FindFiles` or `SearchCode`
  respectively, because the Bash-search branch unregisters Glob/Grep and such a profile would
  otherwise be left with Bash `find`/`grep` only. An explicit `disallowedTools` entry still wins.
  The shared Agent tool description stays host-neutral (no FindFiles/SearchCode).
- Agent listing (2026-10-31): the per-session "Available agent types" reminder now names
  FindFiles/SearchCode for Explore, and for custom profiles that list Glob/Grep, when the
  session's own tools include both. In Bash-search mode those profiles' Glob/Grep, which are
  unregistered, are dropped from the line. Before this the listing said Explore had only
  "Read, Bash, …", so the main model wrongly concluded Explore had to search with `rg` and did
  the work itself instead of delegating. Runtime tools were already correct; the CLI log showed
  Explore calling SearchCode. Sessions keep the listing they announced first, so only new
  sessions see the change.
- Owner: the CLI process (`bootstrap/src/zcode-protocol/code-search-manager.ts`). One fff
  index per workspace key, shared by every session, subagent and workflow agent in the
  process. It opens on the first query and closes once the workspace has had no active agent
  for 60 seconds (checked every 5 seconds: active or queued turns, background tasks and
  workflows, pending permission/question prompts, queued commands; an idle open chat does
  not count). The next query reopens it; cursors from a closed index report "expired".
- Packaging: `@ff-labs/fff-bun` (MIT) is external in the CLI's esbuild/Node build and is
  embedded, with its native library, by `scripts/build_backend.ts` (`bun build --compile`).

## Noise-free transcript (2026-10-19, GPUI-only)

- No single Electron counterpart. Electron has two separate switches: `messageStreamShowReasoning`
  and `messageStreamShowTodos`. GPUI adds a filter button to the composer's bottom row, next
  to the mode menu (the phone gets it too, through the shared composer). It is **on by
  default** and stored as `gpuiNoiseFree` in `gpui_settings.json` (absent = on) through
  `PreferenceChange::NoiseFree`.
- Rows shown while it's on: user input, turn headers, assistant text that has text, thinking
  that has text, and tool cards for `TodoWrite`, `AskUserQuestion` and `ExitPlanMode`, plus any
  tool waiting on `pendingApproval`. Permission and question prompts replace the composer and
  are never affected. Other tool calls, and subagent cards not paired to a tool call, fold.
  Rows that render nothing (empty thinking, empty text, paired subagent rows, unknown kinds)
  don't break a run.
- Thinking that has text shows **open** while it's on (2026-10-20): under the usual
  "Thought · N seconds" header, the full text (no height cap, live while streaming) in muted
  italic on a faint panel-colored block, so it reads apart from responses. Clicking the
  header closes it. With the mode off, thinking stays closed by default as in Electron, and
  opened text is capped at 240 px. `RootView.toggled_reasonings` holds the rows the user
  flipped against the current mode's default.
- Each run of folded rows renders one muted line on its first row: "N tool calls", with
  "· K running" and a spinner while any of them runs, and "K failed" in red. Clicking it
  expands that run in place ("Hide N tool calls"). The expanded state lives per window in
  `RootView.expanded_folds`, keyed by the run's first row id. The list index is unchanged
  (`transcript/noise_free.rs` places each visible row by walking its own run), so history
  paging and scroll anchoring work as before.

## System prompt override (2026-10-19, GPUI-only)

- No Electron counterpart. Settings › System prompt (desktop only, hidden on the phone)
  lets the user replace the main agent's system prompt. It uses the CLI's existing
  `customSystemPrompt` path: the text replaces the default body and every dynamic system
  section (environment, git status, memory guidance, desktop context, communication and
  autonomy text). The CLI always keeps the first line "You are ZCode, an interactive coding
  agent", the skills listing, AGENTS.md/MEMORY.md and the date. Subagents and workflow
  child agents never take the override.
- Owner: GPUI. The override is stored in `~/.zcode/v2/gpui_system_prompt.md`, not in the
  shared `setting.json` (Electron rewrites that file and drops unknown keys). A missing or
  blank file means the default. It applies to every workspace.
- Protocol (new, GPUI is the only caller): `workspace/readSystemPrompt {workspace}` returns
  `{defaultPrompt, defaultSource: "session"|"static", override}`, and
  `workspace/updateSystemPrompt {workspace, prompt: string|null}` returns `{override,
  updatedSessionCount}`. Both use strict schemas. Null or blank means reset. The CLI keeps
  the value in memory only (`appRuntimePreferences.systemPrompt`), so new and resumed main
  sessions read it in `createRecord`, and resident main sessions get
  `runtime.setSystemPromptOverride`. That rebuilds the prompt prefix right away when idle,
  or when the active turn ends.
- Order: on `startup/storageState: ready` GPUI sends `updateSystemPrompt` only if an
  override exists. Entering the section sends `readSystemPrompt` to the active workspace
  agent. Apply/Reset write the file first and then send `updateSystemPrompt` to every
  started agent. An agent that answers -32601 (for example the installed Desktop runtime
  fallback) marks the feature unavailable without an error banner.
- Editor rules: it is prefilled with the override, or else the default. While the user
  hasn't edited it, it follows later commits (default arriving, Reset). Apply is disabled
  while the text equals the committed value. Text that is blank or equal to the default
  applies as a reset, so an untouched editor never freezes a copy of the default. When no
  session exists yet, the default is only the static identity text, and the page says to
  open a chat to load the full one.

## Voice slice (2026-10-16, GPUI-only)

- New GPUI-only feature with no Electron counterpart: a `Speak` tool lets the main agent talk
  to the user aloud on the PC (Kokoro TTS in the `apps/zcode-voice` sidecar), plus fallback
  lines for permission, question and plan-approval prompts. Settings › Voice is desktop-only.
  The full spec is [VOICE.md](VOICE.md).

## Long paste, composer caret, tok/s and phone touch slice (2026-10-16)

- **Long paste becomes an attachment.** As in Desktop (`shouldCreateClipboardTextAttachment`),
  pasting text of at least 15 × 1024 UTF-16 units into the main composer does not insert it.
  It becomes a `text/plain` attachment named `pasted-text-YYYYMMDD-HHMMSS.txt` (local time).
  The chip reads "Pasted text" with "N lines" (`\n`, `\r\n` and `\r` each end a line). In a
  session the text uploads through the same `v4/attachment/*` transaction as clipboard images
  (Uploading / Retry / Cancel). In a new-task draft it is written to an owned paste temp file,
  like a draft image. Shorter text, single-line inputs (rename, commit message) and edit or
  rename intents insert the text as before.
- **Click moves the caret.** A plain click in the composer places the caret and clears the
  selection. Before, it extended a selection from the old caret to the click. Drag selects from
  the click point, and Shift+click extends from the existing caret.
- **Phone keyboard.** On the phone build the soft keyboard appears only when the user taps
  the composer. Startup, closing Settings, the command center or a subagent view no longer
  focus the composer there. A touch outside the composer block (input, tray, footer buttons
  and the autocomplete popup) while it is focused removes the focus, which hides the keyboard. The platform layer re-shows a dismissed
  keyboard only after a real tap (click count ≥ 1), not after a scroll.
- **tok/s times the whole model call.** The exact rate is a call's output tokens over the
  time from the request being sent to its usage event. The request counts as sent at the
  latest of: the turn starting (`turnHeader` or `userInput` row appended), the previous
  call's usage event, and a tool call reaching `success`, `error` or `cancelled` before
  this call's first output. Tool runtime (a long bash command, a subagent) therefore stays out
  of the window, and tool results finishing while the model still streams don't shorten it.
  Before, the window opened at the first output row. Proxied Claude models stay silent for
  6–16 s and then send the answer in about a second, so the old window showed 200–1200 tok/s
  for calls that really ran at 40–90 tok/s (`model_usage` first-token and completion times).
  The live `~` estimate uses the same start. A usage patch with unchanged output tokens (model
  or context-window change) is not a call boundary. The first usage after load is only a
  baseline and opens no window, because a call may already be in flight.
- **Scrolling doesn't click.** When a touch turns into a scroll, a long press or a
  multi-finger gesture, `gpui_android` sends the cancelling left-button release off-window.
  gpui fires `on_click` for any release over the pressed element whatever its click count, so
  a scroll that started on a project folder toggled that folder.

## Context ring and tok/s meter (2026-10-15)

- **Context ring.** As in Desktop's model controls, the composer footer starts with a 14px
  ring (28px ghost button, `chat-context-usage-trigger`) showing `usage.contextWindow`
  `usedTokens / maxTokens`, clamped to 0–100%. The track is the subtle color at 25% opacity and
  the arc at 70%, starting at 12 o'clock and running clockwise with round ends. It is hidden when
  the window is null, zero or invalid. Snapshot and `state.updated` usage replace it whole.
- **Usage card.** Hovering the ring opens a 320px card: "Context windows" with the compact
  summary (`12.3K/200K (6.2%)`, 万/亿 in Chinese), a bar filled to the used share and split by
  the breakdown sources (largest first, ties in Desktop's source order, chart color fading toward
  the surface), one row per source with its share of the breakdown, and "Average cache hit rate"
  only at 78% or more. The Coding Plan and Start Plan panels are not ported.
- **tok/s.** The exact rate divides a call's output tokens by the time from its first model
  output to its usage event. The first output is the first appended streaming reasoning or text
  row, or `inputStreaming` tool row, so signature-only thinking counts toward the time. Before,
  the window started at the first visible delta, which made a 6 s thinking-plus-tool call read
  as over 1000 tok/s. Subagent `summaryText` deltas no longer feed the estimate.

## Browser Use bridge slice (2026-10-15)

- **Browser tool works through your own browser.** `interaction/browserList` returns one
  `type: "extension"` descriptor per connected Helium/Chrome profile, and `browserExecute`
  runs in that browser through the ZCode extension and `zcode-browser-host`. With no
  browser connected the replies are unchanged (`{browsers: []}`, fast failure). Spec, owners,
  event order and security: `BROWSER_BRIDGE.md`.
- **Shared command layer.** Desktop's Electron-free browser commands moved unchanged into
  `packages/browser-commands`; Desktop and the extension both use it.
- **Settings › Browser Use** (Agent capabilities group) shows the bridge address, connected
  browsers and extension version, registers/removes the native host (Windows HKCU), and
  opens or copies the unpacked extension folder. Hidden in the phone build.

## Composer, tool image and sidebar slice (2026-10-15)

- **Working indicator.** While the session phase is running, the composer shows Stop with a
  spinning accent ring (tooltip "Agent is working · Stop"). As in the Electron composer, an
  empty draft shows only Stop; with a draft (text or attachments), Send (queue or steer) sits
  beside it. Idle shows only Send.
- **Tokens per second.** Output tokens include thinking, which often streams no text (Claude
  sends signature-only thinking). The exact rate's window therefore starts at the earlier of
  the first visible delta and the response's reasoning start (reasoning row `durationMs`
  before the row reported it), not at the first visible text. A long silent think followed
  by a short sentence read as thousands of tok/s before.
- **Tool result images.** Thumbnails keep the image's aspect ratio inside 320 × 240 and never
  enlarge. Clicking one opens a full-window viewer: fitted to the window, or "Actual size" 1:1
  in a scroll area (click the image or the button to switch). Esc, Android back, "Close" or a
  click on the backdrop closes it. Thumbnail element ids include the row id, since one file is
  often read many times in a session.
- **Sidebar footer.** The status text line is gone. The Settings row stays at the bottom with a
  dot on its right for the active workspace's agent: green connected, amber starting,
  restarting, connecting or pairing, red failed or offline, grey idle (not started yet). Its
  tooltip is the former text (`<launch description> · <status>`).
- **Project folders.** A header click toggles the folder (Electron `handleHeaderClick`);
  opening a folder that isn't the active project also activates it so its agent loads the
  sessions. A folder opened by a click or by becoming active stays open when another project
  becomes active, until the user closes it (`sessions/folders.rs`).
- **Project list (2026-10-21).** The Projects section lists every local project in
  `setting.json`, uncapped: `lastActiveWorkspace` first (so it is the startup project), then
  `lastWorkspaceSession` (the desktop's projection of every non-deleted local project root,
  in its order), then `recentProjects`; deduped by canonical path, missing folders, remote
  entries and the Tasks directory skipped (`backend/workspace_paths.rs`). The launch cwd is
  not a project; only a folder passed on the command line joins the list. The old 8-entry
  cap plus the cwd slot hid projects (including the last active one); agents start lazily,
  so a long list costs nothing until a project is opened.
- **Project order (2026-10-27).** The sidebar shows projects with the most recent task
  activity first: the newest `lastActivityAt` among the project's listed tasks (subagent
  sessions excluded), so starting a task or continuing one moves its project to the top.
  While a task is running its sort time stays at the moment the run started (2026-10-28):
  `lastActivityAt` moves on every agent step, so two busy projects kept swapping places.
  Each session's sort time (`SessionEntry::sort_activity_at`, set in `backend/route.rs`)
  only advances when a run starts (the user's message) or when the session is not running
  (the run finished); the same key orders tasks within a project. The row's relative time
  still shows `lastActivityAt`.
  Projects without tasks follow in discovery order; ties keep discovery order. This is a
  render-time sort (`sessions/items.rs` `sort_by_latest_activity`); the workspace list
  itself keeps the discovery order above, which picks the startup project.
  A project's task list only arrives once its agent starts (startup project, or first
  open), so the newest activity seen for each project is kept in
  `~/.zcode/v2/gpui_project_activity.json` (`shared/project_activity.rs`, GPUI-owned,
  `{ canonical workspace key: ms }`). It is loaded at startup; a project sorts by the later
  of its live list and the remembered value, so projects whose list hasn't arrived yet keep
  their last known place (`app/project_order.rs`). Every sessions-index snapshot
  or delta that moves a project's newest activity forward updates it, and the maintenance
  sweep writes it to disk (atomic temp-file rename) when it changed. Activity made outside
  GPUI in a project GPUI hasn't opened this run shows up once that project is opened.
- **Shell conveniences (2026-10-27).** Ctrl/Cmd+B hides or shows the sidebar (in-memory, not
  persisted); Ctrl/Cmd+= / - / 0 step the interface font size by 1px within the settings
  range and reset it, through the same `FontSize` preference as the slider; Ctrl/Cmd+W
  closes the side pane when it is open; Ctrl/Cmd+Shift+[ / ] go to the previous / next task
  in sidebar order, wrapping (`app/shortcut_actions.rs`). Back / forward (Ctrl+[ / ]) stay
  unwired: GPUI has no navigation history. Error banners and the turn error wrap, scroll
  past about six lines, are selectable and have a Copy button (`app/error_banner.rs`). A
  round jump-to-latest button floats over the transcript while it is not following the
  bottom; clicking it scrolls to the end and resumes following. Files can be dropped
  anywhere on the chat panel (transcript included), which tints while dragging. Sidebar task
  rows show the full title as a tooltip, and Rename / Delete have tooltips.
- **Project context menu (2026-10-21).** Right-clicking a project row opens New task (same as
  the row's "+"), Open in File Explorer / Finder / File Manager (the folder itself), Open in
  editor (VS Code, else the system opener), Copy path, and Remove from sidebar
  (`sessions/project_menu.rs`, ely `ContextMenu`). Remove stops the project's agent, drops
  its local session mirrors and drafts, moves the active project to the next one (or Tasks),
  and forgets the path in `recentProjects`, `lastWorkspaceSession` and `lastActiveWorkspace`.
  Nothing on disk is deleted. It is refused while the project has live work (running turn,
  background work, pending question), since stopping the agent would end it. The desktop's
  pin/edit/delete project actions need its host-owned `tasks-index.sqlite` and are not
  offered.
- **Preload.** The first time a workspace's session list arrives in a run (app start for the
  startup project, first open for each other project), its 3 latest sessions are subscribed
  in the background, so opening one shows its transcript at once. Later snapshots (reconnect,
  respawn after idle unload) don't preload again; sessions already subscribed or with a
  subscribe in flight are skipped (`backend/preload.rs`).
- **Conversation release (2026-10-30).** A conversation subscription keeps that session
  resident in the agent, and each resident session holds its own set of stdio MCP server
  processes. GPUI used to subscribe every thread it opened and never unsubscribe, which reached
  about 150 MCP processes. Desktop releases a thread 30 s after it leaves the screen
  (`SESSION_DATA_LAYER_KEEP_WARM_MS`). GPUI matches it with the 30 s maintenance sweep
  (`app/conversation_release.rs`). A subscribed conversation is released once it has stayed
  releasable for at least 30 s. It is releasable when it is not the selected thread or the open
  subagent view, has no live turn, pending interaction, queued input or running background
  work, and its workspace has no phone viewer. Releasing sends `v4/conversation/unsubscribe`
  and drops the route cursor and fragment state. The local mirror stays with
  `subscribed = false`, so reopening the thread resubscribes it and its snapshot replaces the
  rows. Preloaded threads that are never opened are released the same way.
- **Lazy MCP servers (2026-10-30, CLI).** stdio MCP servers no longer start with the
  session. The agent's connection pool wraps them (`adapters/src/mcp/lazy-stdio.ts`). Tool
  lists come from `~/.zcode/cli/mcp-tool-cache.json`, keyed by server name and a hash of the
  config. A server only stays running after a real use: a tool call, a resource read or a
  panel connection. With no cache entry, it starts once to list its tools and stops as soon as
  it answers. Sessions starting together share that one probe. A Settings › MCP refresh
  probes the same way. After a real start, a changed tool list bumps
  `toolListRevision`, and tools are re-registered at the next turn. Official plugin servers
  (run through the plugin host) are stopped after 5 idle minutes. They are never stopped
  while a panel or resource subscription is attached. User servers are only started lazily,
  never stopped, because they may hold state. `node_repl` stays eager. Official plugin
  servers also default to `isolation: "workspace"`, because this fork's seed manifests don't
  declare it.
- **Cached rows and warm agents (2026-10-29).** A list only arrives from a workspace's own
  agent, so without help the Tasks section stayed empty until "+" started its agent, and an
  expanded project showed nothing until clicked. GPUI keeps the newest 25 rows of every
  workspace, plus the set of open project folders, in `~/.zcode/v2/gpui_sidebar_cache.json`
  (`shared/sidebar_cache.rs`, GPUI-owned). At startup the cached rows seed each workspace
  that has no live list yet; the first live snapshot replaces them (the agent's list is the
  fact; a cache entry never deletes a live row). Startup then spawns, after the primary
  project, the Tasks agent and the agents of folders that were open last run, one every
  ~400 ms (`app/sidebar_warm.rs`); opening a folder still activates it, which starts its
  agent. A background workspace's first snapshot never auto-opens one of its sessions; only
  the active workspace's does (`backend/route.rs`). Each started agent's first snapshot runs the preload above, so the 3 newest tasks
  of every open folder and of Tasks are warm before they are clicked. Idle unload still
  stops agents nobody uses; the cached rows stay visible. The cache is written by the
  maintenance sweep when it changed (atomic temp-file rename).

## Chat usability slice (2026-10-14)

Product rules for this slice. Owners are unchanged: the CLI owns rows, usage and
interaction resolution; GPUI only mirrors and renders, plus local UI state (stepper index,
image cache, composer selection).

- **Plan approval (ExitPlanMode).** A `userInput` interaction whose
  `payload.schema.interaction` is `plan_approval` (or whose tool is ExitPlanMode) renders the
  plan from `payload.input.plan` as Markdown with one-click **Approve**, **Request changes**
  (message-box text as feedback) and **Decline**. Answers use the shape of Desktop
  `buildElicitationResponseContent`: `answers[question]`, `answer_<i>`, and `answer` for a
  single question, carrying option *values*. The broker reads `answer_0`/`answer` because the
  projected question text is the tool's `reason`, not its fixed string. Acceptance:
  Approve exits plan mode instead of "Permission denied for ExitPlanMode".
  Approve with text in the box (GPUI extension, 2026-10-30) also sends
  `content.approvalNotes`; the plan is still approved and the tool result gains a
  "User adjustments" section. An empty box approves exactly as before.
- **AskUserQuestion.** One question at a time (header badge, `n / N`, Back,
  Continue→Submit on the last). Options are full-width wrapping rows. Nothing overflows the
  card at any width.
- **Composer editing.** One wrapped text layout (no horizontal overflow). Selection with
  Shift+arrows/Home/End/drag, Ctrl/Cmd+A, Ctrl+Backspace/Delete and Ctrl+Left/Right by word
  (Alt on macOS), Ctrl/Cmd+C/X copy/cut, typing/paste/IME replace the selection. Since
  2026-10-27 (`composer/history.rs`, `recall.rs`, `lines.rs`, `context_menu.rs`):
  double-click selects a word and triple-click a line (between newlines), and a drag after
  either extends by that unit, keeping chips atomic. Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z (and
  Ctrl+Y off macOS) redoes; at most 100 steps, consecutive typing or deleting is one step,
  uncommitted IME text is not recorded, and sending or restoring a draft clears the history.
  Home/End go to the edge of the wrapped visual line, Ctrl/Cmd+Home/End to the message edges,
  and Up/Down move one visual line. Up in an empty box (or on the first line of an unedited
  recalled message) recalls the previous real user message of the active conversation,
  newest first, from loaded rows only; Down walks forward and past the newest restores what
  was typed before. Right-click opens Undo/Redo/Cut/Copy/Paste/Select All, disabled when
  they cannot apply.
- **Transcript text selection** (`shared/selectable.rs`). User messages, Markdown blocks
  (keeping their styling and links), whole code blocks and expanded tool-card text (fields,
  Bash command, edit path, output, task output) select like Desktop's browser text: drag
  (also past the element's edge), Shift+click, double-click word, triple-click block,
  Ctrl/Cmd+C/A while focused; clicking elsewhere clears it. Since 2026-10-27 a Markdown
  document has one selection (`shared/selection_group.rs`): a drag runs across paragraphs,
  list items, headings, table cells and code blocks in reading order, Ctrl/Cmd+C joins the
  blocks with newlines and Ctrl/Cmd+A selects the whole document. Dragging after a
  double-click extends by words, after a triple-click by blocks. A double-click on the
  right half of a word's last letter still selects the word. Other selectable text (user
  bubbles without Markdown, tool-card fields, thinking blocks, subagent summaries, error
  banners) stays one block each. A diff view is one selection with one block per line;
  copying leaves out the +/- gutter. Escape clears a selection (and only then stops the key).
  Right-click offers Copy (disabled without a selection) and Select All; Markdown adds
  "Copy as Markdown" and diffs "Copy patch". On Android a one-finger drag still scrolls; a long press selects a word.
- **Tool cards.** No raw JSON by default: AskUserQuestion, ExitPlanMode (Markdown), TodoWrite
  (checklist), Agent/Task (description + prompt), TaskOutput (status + output, from the
  `task_output` display or the tagged text), otherwise a key/value view. Raw text only while
  input is not valid JSON. Outputs drop `<system-reminder>` blocks and reduce TaskOutput and
  `<task-notification>` wrappers to their readable body. Background-result user rows render as
  a notice card, not XML. An approved ExitPlanMode result keeps only its status line: the CLI
  appends the whole plan again after `## Approved Plan:`, which the card already renders from
  the input (desktop's plan card shows the plan, not the result echo). A TodoWrite result that
  echoes the list as JSON (`oldTodos`/`todos`/`summary`) is not printed under the checklist;
  any other TodoWrite result, such as an error, still is (desktop `todo.tsx` never mounts the
  output when it has a plan).
- **Nested lists in Markdown (2026-10-21).** A tight list item's text may be followed directly by
  a nested list. The inline walker stops at any block-level start, so the nested list renders
  as its own indented list, the parent numbering is untouched, and nothing after it is dropped
  (previously the plan card stopped rendering after the first nested bullet list).
- **Images.** Tool results show inline images from `display.images` (`node_repl_images`) and
  `display.media`, and images read by the Read tool from absolute local paths. Images are
  decoded in the background with the Files-pane limits, cached by source, and enlarge on
  click. User attachments show name/size chips only: rows carry references, not bytes.
  Finished assistant replies show the same thumbnails below the text for Markdown images
  (`![alt](path)`) that point at absolute local paths or `file://` URLs with an image
  extension (2026-10-27); the inline `[image: path]` link stays. Relative paths and remote
  URLs are not loaded. The Desktop context prompt tells the model to use this syntax.
- **Reasoning.** Investigated: main-session Claude models return mostly signature-only
  thinking (85% empty blocks in the model-io log), while subagent models return full text.
  GPUI is correct to mirror empty rows. An expanded empty block says the model did not share
  its reasoning.
- **Thinking level.** Brain icon plus a vertical success-coloured bar filled by the level's
  position among non-off levels (Desktop `ThoughtLevelCycleControl`). Clicking it cycles
  levels; the label opens the menu.
- **Timestamps and throughput.** User and assistant rows show local `createdAt`
  (`HH:MM`, plus the date when not today). The composer shows output tokens/s: `~` estimate
  from streamed characters while a response streams (usage arrives only at ModelComplete),
  then the exact `usage.cumulative.outputTokens` delta over the streaming time.
- **Identity.** The window title, draft greeting and Settings › General state that this is an
  unofficial community client, not an official ZCode product.
- **Phone remote control:** not shipped yet. It is planned as a local-network-only native
  Android build of this app talking to the desktop instance; spec and build-spike results
  are in [REMOTE_CONTROL.md](REMOTE_CONTROL.md). The Desktop relay implementation
  (`packages/desktop/src/main/webRemoteControl*.ts`) is not ported.

## v3.15.1 sync and Bun runtime rule (2026-10-13)

The branch is rebased onto upstream v3.15.1 (`feat/gpui-v3.15.1`). **The owner replaced
the blanket backend freeze with a Bun runtime rule:**

- **Allowed:** changes that let the existing CLI app-server and Services Host load, run and
  build under Bun: runtime shims (sqlite, `node:sea`, worker threads, PTY degradation),
  Bun build scripts replacing `tsx`/Node-targeted esbuild, and source-mode entry points.
  Each must feature-detect and leave Node/Electron behavior unchanged.
- **Still forbidden:** protocol/RPC surface changes, new service APIs, service business
  logic and Electron desktop changes. A missing owner port is still a blocker, not a
  reason to invent one.
- Prefer GPUI-owned entry scripts over editing upstream files when either works; upstream
  ships one squashed snapshot per release, so every backend edit is a future rebase conflict.

Current results under this rule:

- The full Services Host runs under Bun from source through `scripts/services_host.ts`,
  which supplies the build-time version and production Built-in Provider release before
  loading the unchanged `packages/server/src/entry-stdio.ts`. No backend file changed.
  `services_launch.rs` prefers it, keeping env overrides and the installed-Desktop bundle as
  fallbacks. All ten ignored real-Host tests pass with the prebuilt bundle removed; measured
  working set is about 156 MiB (Desktop runtime: 190–194 MiB).
- Subagents opt-in is persisted in a GPUI-owned `gpui_profile_consent.json`. Entering
  Settings › Subagents starts the Host; leaving or 60 s idle stops it; Resume is one click.
- The native executable (isolated, populated) rendered the inventory and the edit form
  (tools, model, color, AGENTS.md) with both backends running as `bun.exe`. Visual
  parity with the Desktop dialog is not accepted.
- Compiled backend prototype (bun 1.4.3): `scripts/build_backend.ts` compiles the unchanged
  CLI app-server into `zcode-backend[.exe]`; GPUI prefers it when it sits beside the GPUI
  executable. Three feature-detected backend edits (self re-exec, provider-release lookup,
  ripgrep worker import) were needed. The full backend smoke passes against the binary, and an
  isolated native run spawned it and completed startup and `session/create`. Idle memory:
  176 MiB working set / 277 MiB private, versus 251 / 872 MiB for bun running the source.
  Official plugins are not staged yet (see README).
- `bun run typecheck:fast` is an advisory `bun check` pass; tsc remains the gate.

New upstream v3.15.1 surface **not yet in GPUI**: plugin UI and its desktop plugin sandbox
(`packages/ui/src/plugin-ui`, `packages/desktop/src/main/pluginSandbox`), inline Gen UI
(`packages/ui/src/gen-ui`), workflow-timeline and ai-elements components, highspeed
model badges, cloud-content and manual-claim-plan dialogs. Bot channel replies and topic
resources (Feishu/Lark relay) are excluded and fail fast (B9). Host-resolved subagent
runtime snapshots (B10) are not supplied; GPUI sessions use the CLI's own Markdown
profile loading, so whether Settings model overrides reach GPUI-launched sessions is
unverified.

## Approved real-management extension (2026-10-06)

The native-only slice implements opt-in local Subagents management with the unchanged
full Services Host and rebuilds the rejected inventory using the Desktop reference. Actual
native visual acceptance remains unverified; see SETTINGS_ACCEPTANCE.md.
SERVICES_RPC.md defines preference suspension/drain and observed Host exit before writes
resume. SUBAGENTS_SETTINGS.md defines activation, recovery and compact grouped/inline UI.
Earlier isolated evidence is not acceptance of this extension. Backend stayed frozen at the
time (superseded 2026-10-13 by the Bun runtime rule above); no real configuration is used during automated/native acceptance. Existing migrations/metadata
rewrite and unsupported cross-process coexistence are disclosed, not claimed solved.

## 1. Goal, scope, guardrails

**Stack note (2026-10-03):** `gpui` is now pinned to the exact zed revision that
`ely-gpui-component` targets (git rev `1a28cff`, 2026-09-27) so both build one shared
gpui; `gpui_platform::application()` replaces the removed `Application::new`. ely is
initialized in `main.rs` with its assets/fonts. The serialized preference owner applies
Ely mode/font scale and the legacy semantic color/typography adapters together. New
Settings controls use Ely directly; legacy views remain incrementally migrated.

**Goal**: the GPUI client is a daily driver that a ZCode user can run _instead of_ the
Electron app for coding work, at a fraction of the memory, without forking the backend.

**Guardrails** (unchanged from v1, still binding):

1. **Zero backend forks.** Only wire surfaces that exist in
   `packages/shared/src/zcode-protocol*` (V4 topics/commands + legacy method registry).
   Runtime-portability edits for Bun are allowed under the 2026-10-13 Bun runtime rule
   (e.g. `node:sea` probed through `process.getBuiltinModule` in
   `apps/zcode-cli/packages/core/src/environment.ts`). They touch no wire protocol.
2. **Ownership stays put.** Agent lifecycle, CommandInbox admission, task leases and
   the remote registry belong to the backend/host; the frontend mirrors state, keeps
   drafts and renders optimistic UI.
3. **Every milestone ships a usable app** that connects to the real backend and
   completes a full conversation round.
4. Repo policy: files ≤ 400 lines, Apache-2.0 only (never copy zed-industries/zed
   GPL code; this repo's own `packages/*` source is fair to read and mirror),
   localized shipped UI, Chinese cause/fix comments for bugs per AGENTS.md,
   `cargo fmt --check` + `clippy --all-targets -D warnings` + `cargo test` green.

**Out of scope for parity** (decided, revisit only with a new requirement):
computer use (CUA helper, macOS TCC), remote SSH/WSL/Docker workspaces, the Desktop
phone relay (a LAN-only replacement is planned in REMOTE_CONTROL.md) / bots, conversation share, whiteboard, treemapping, model-trajectory and developer-tools
panes, feedback center, resource-manager window, coding-plan purchase webview, ARMS
telemetry. Do not expose excluded capabilities as functioning controls; an explicit
unavailable message or desktop handoff is required at any future entrypoint.

## 2. Current action-level status (2026-10-06)

Historical “COMPLETED” labels counted models/helpers as user-facing parity. The table
below separates reachable functionality from remaining work and runtime evidence.
Tests alone do not establish native visual acceptance or full end-to-end parity. Current
native-only Settings results are in `SETTINGS_ACCEPTANCE.md`; `SETTINGS.md` also retains
earlier slice evidence. Subagents now has deliberate local activation and canonical
management, with native preference handoff and observed process-tree exit. The rejected
inventory was redesigned; actual visual parity and complete native actions remain unverified.

Reference Settings navigation currently offers General, Appearance, Model settings,
Browser Use, Keyboard Shortcuts, Memory, Subagents, Plugins, MCP Servers, Skills,
Commands, Hooks, and Usage stats. Hidden legacy/Computer Use/workspace-file-search
sections are not reachable reference navigation. As on Desktop (`settingsPageConfig.ts`,
`CONTEXT.md` "商店页结构"), the plugin store (Public / Personal segments, Installed Strip,
Manage Installed) is the Settings Plugins page, and MCP Servers and Usage stats are
Settings-only; entering Settings Plugins lands on the Public segment. The right dock keeps
only Review, Files and Workflows (saved workflows stay there until an Automations
Settings section exists). Applying an Example Prompt closes Settings so the prefilled
draft is visible.

| Area                              | Current status and remaining boundary                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Transport and routing             | Implemented fail-closed subscription/epoch/sequence rules, bounded assembly/backpressure, and process-generation isolation. Golden tests cover malformed/stale/overflow cases. CLI smoke verifies live/replayable frames and recovery; full GPUI executable transport/render smoke remains open.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Chat and rendering                | Basic chat, interactions, supported message/session actions, Markdown/code/diff rendering and virtualization are reachable. Drafts survive navigation in memory only. Complete reference action parity and performance measurements remain unverified.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Submission safety                 | Failed enqueue reconciles pending state and restores composer ownership. Explicit rejection recovers the originating draft; uncertain ACKs do not auto-resend. Create ACK activation is navigation-generation guarded.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Review/files/terminal             | Basic local panes and bounded static raster image previews are reachable. Files preview/tree replies check workspace identity and selection/reset generations; measured clicks exercise real background fixture IO and image atlas upload. Terminal scrollback drag selection and Ctrl/Cmd+C copy use alacritty's selection owner; Ctrl+C without selection retains ETX. Platform-appropriate PTY computed-output smoke passes on this Windows checkout; native image visual acceptance, macOS/Linux execution and long-tail actions remain unverified.                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Reverse RPC and launch            | `--surface desktop`, explicit responses/races/fast failures, cleared allowlisted environment, and unified roots are implemented. Account auth, official MCP auth and host scheduling are not supplied; browser execution goes through the extension bridge (`BROWSER_BRIDGE.md`). Current CLI reverse callsites have an automated inventory gate.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| Composer/catalog/queue            | Aggregate activity opens running/ended agents and read-only child chats; live scoped skill/plugin catalogs, queue reorder/Edit restoration and availability-gated goal Pause/Resume are reachable. Local-path attachments have explicit deletion ownership. One-shot modified delivery and plain-input/new-goal held-queue Clear/Keep/Cancel are reachable with current-owner/reviewed-ID guards and real-CLI held-queue smoke. CLI-owned draft/session slash discovery, `/init` selection and custom filtering are reachable without local fallback or prompt copies. Local `/plan` prepares an in-memory override or submits a task through ordinary input, with selector/rejection/held-queue guards. Existing-chat clipboard uploads use begin/chunk/commit with bounded source ownership and reachable Retry/Cancel; stale replies cannot attach to another composer. New-chat non-path uploads, other app-only slash routing and native race/durability evidence remain partial. |
| Settings/appearance/shortcuts     | Grouped/searchable gear/command/shortcut destination, direct-section intents, General/Appearance and complete binding vectors for supported native shortcuts are implemented. Serialized production preferences preserve unknown JSON; isolated preferences are read-only. Model settings is the desktop two-column provider manager over `providerSettings/read|mutate` (`PROVIDER_SETTINGS.md`); coding-plan/account panels and model metadata editing are not ported, and agents without those methods get the read-only catalog. Keyboard/pointer/recorder/stream/re-hydration fixtures pass; accepted native visuals, complete locale/theme coverage and executable restart remain open.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Remaining Settings sections       | Plugins store and Manage Installed, MCP status inspection/Open authorization and workspace/range-scoped Usage are Settings destinations. MCP authorization uses a secret-safe native opener with current owner/connection/query checks; configuration CRUD is not supplied. Memory is reachable through the guarded application preference owner and committed reverse runtime replies. Configured-model inspection is read-only. Subagents uses the unchanged full Services Host in disposable acceptance or explicitly activated local management with suspended native preference writes. Its reference-style redesign is not yet visually accepted. Browser Use is implemented (`BROWSER_BRIDGE.md`). Skills, Commands and Hooks still require missing owner ports or further bounded work. No unused-field toggles are presented as supported.                                                                                                                                                                    |
| Workflows/MCP/usage/subagents     | Timeline/resume and saved project/global list, typed new-chat launch, definition inspection, run history/Open chat and eligible-run model/concurrency settings are reachable. Metadata editing and confirmed Delete/global-to-project Move are reachable with fresh-definition preflight and origin-only refresh. Projected Open run/Open successor/All runs and exact stream-gated amendment following are reachable. Focused-run artifact metadata/Refresh uses the existing journal query with parent/run/process/selection guards and safe-field decoding. Latest-version Markdown View/Retry/Close uses a bounded authorized read and passive rendering. Other artifact formats, version navigation and cross-project history/detail remain partial. Stream-confirmed settings avoid ACK-derived state. Automations/off-peak stay host-owned and fail fast.                                                                                                                       |
| Plugins                           | Store/install/enable/update/uninstall/restore/remove and Personal Source Add/Manual Refresh use captured workspace, one mutation sender and pending guards. Diagnostic errors and refreshFailure are visible; Add retains its local draft and uses a guarded directory picker. Personal Source plugin cards and guarded canonical Example Prompt prefill are reachable without destroying parent drafts. Read-only Details/Refresh and typed scoped Configure/explicit Clear/confirmed Reset use existing CLI ports with preflight, secret masking and origin-only refresh. Workspace Reset preserves options; user Reset removes its configuration. Remote-sync cancellation is excluded, not a fake local Stop. Full lifecycle and native evidence are not complete.                                                                                                                                                                                                                 |
| Task metadata and synchronization | Loaded session navigation and active-tail polling exist. Full host task index, pin/archive/unread and background cross-process synchronization are missing; do not invent frontend authority.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Native lifecycle                  | Windows single-instance scaffold, minimal notifications/keep-awake and log export exist. Activation forwarding, tray, multi-window and portable lifecycle validation remain partial.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Distribution                      | Version/manifest/packager scaffolding; optional compiled sidecar backend (`package:gpui --with-backend`, prototype, official plugins not staged). No installer, signing or update installation path.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Framework/visuals                 | Git-pinned GPUI/Ely assets and components are integrated. Live semantic theme/typography adapters cover migrated surfaces; this is not a native visual acceptance claim.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

## 2a. Remaining parity boundary (2026-10-06)

_Superseded in part on 2026-10-13: Bun runtime-path edits are now allowed (see top); the
ban on new APIs, service logic changes and parallel writers below still holds._

The owner clarified that the existing TypeScript backend must remain intact. The proposed
configuration-service worker and canonical service/repository changes were withdrawn;
this slice may change native frontend code, tests and specs only. Existing CLI/runtime,
service persistence, provider repositories and shared protocol implementations are not
modified to close native UI gaps. Subagents remains the highest-priority missing panel;
SUBAGENTS_SETTINGS.md records isolated implementation, rejected appearance and production gates. A
native UI must reach an existing unchanged management owner before offering writes; the
session/subagents child directory is not that owner. No new RPC claims, parallel config
writer or functioning-looking management controls may conceal the missing boundary.
Other exclusions remain binding (browser execution was lifted on 2026-10-15; see `BROWSER_BRIDGE.md`).

The unchanged Desktop service connection requires an Electron-transferred MessagePort;
it has no supported external GPUI attach path. Existing headless stdio reaches the
service only through a full Host with eager provider initialization and conditional
migration, not a narrow/lazy settings mode. SUBAGENTS_SETTINGS.md records source-backed
assessment and rollback verification. Opt-in local management accepts the full Host's
disclosed behavior; standalone packaging, concurrency safety and visual acceptance remain separate.

The native-only follow-up uses a Rust Services client with the unchanged full Host.
Disposable --isolated-settings remains the sole acceptance mode; ordinary local management
requires activation consent and preference-owner handoff. SERVICES_RPC.md and
SUBAGENTS_SETTINGS.md define transport, owners, forms and production gates. Backend code
remains frozen. Grouped navigation, multi-binding shortcuts and configured-model inspection
are implemented independent native slices; neither generic models nor child chats are
profile settings. Real Host tests, correlated same-CLI-server schema snapshots and actual
native creation/supported-field readback after same-executable restart pass in disposable
roots. Inspected dark/English and light/Chinese narrow captures demonstrate the redesigned
structure; full native actions and reference visual parity remain unverified. Measured
full-Host cost does not establish the lower-memory production goal.

## 3. The real blockers v1 missed

Researching the host layer (`packages/services/src/zcode-agent/zcodeAgentService.ts`,
`zcodeAgentProcessManager.ts`) shows the gap is not mostly UI. The desktop host serves
**agent → client requests** and sets **launch parameters** that change backend
behavior. Known reverse requests are now explicitly served, raced, or failed fast;
only unknown methods receive `-32601`. The current boundaries are:

| #   | Host duty (desktop source)                                                                                                                                                                                              | Effect of `-32601` / omission in GPUI                                                                                                                                                                                                                                                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | `interaction/requestProviderRuntimeHeaders` (`zcodeAgentService.ts:2245`); the CLI asks before **every** model request on account providers (`bootstrap/src/zcode-protocol/provider-runtime-headers.ts`, 180 s timeout) | Zai coding-plan / account-login users cannot chat; direct API-key providers are unaffected (they never enter this port). Deferred to M11. In `app-server` mode the CLI's standalone credential port is **not** used; it only exists for `prompt`/TUI (`process-provider-registry-runtime.ts:55`). |
| B2  | `interaction/requestOfficialMcpAuthHeaders` (`:2296`)                                                                                                                                                                   | Official MCP servers that need account auth fail.                                                                                                                                                                                                                                                 |
| B3  | `session/requestRuntimePreferences` (`:2113`)                                                                                                                                                                           | Memory reads the committed application preference (default false) and has a functioning Settings control. Search/AskUserQuestion retain fixed defaults. Create/resume materialization freezes preferences; active runtimes are not reset.                                                         |
| B4  | `automation/{create,update,list,delete,checkTaskBinding}`, `offPeak/{create,list}` (`:2491-2810`); **the agent asks the host**, and the host owns the scheduler (`packages/desktop/src/main/desktopCronScheduler.ts`)   | Agent tools that schedule work fail. v1 M5 assumed these were frontend→agent calls; they are the reverse.                                                                                                                                                                                         |
| B5  | `interaction/browserList`/`browserExecute` (`:2402`)                                                                                                                                                                    | Done 2026-10-15 through the user's browser (`BROWSER_BRIDGE.md`); no embedded browser. Fails fast when no browser is connected.                                                                                                                                                                                                  |
| B6  | Launch arg `--surface desktop` (`zcodeAgentProcessManager.ts:496`) adds the desktop context section to the system prompt (`core/src/context/builder.ts:131`)                                                            | Fixed: launcher candidates pass `--surface desktop`. Prompt identity across two real clients has not been separately recorded.                                                                                                                                                                    |
| B7  | Host env: `ZCODE_RUNTIME_ENV`, workspace identity env (`buildAgentWorkspaceIdentityEnv`), proxy (`resolveSpawnEnv`), `ZCODE_DATA_BASE_DIR`, `GLM_BINARY_PATH` (`desktopRuntimeEnv.ts:466`)                              | Resolved bootstrap/data roots and an explicit cleared-environment allowlist are implemented; identity/proxy/CA boundaries remain covered by launcher/path tests.                                                                                                                                  |
| B9  | v3.15.1 `bot/channel/reply`, `topic/resource/{read,cancel}`: Feishu/Lark bot relay owned by the desktop host | Excluded scope; GPUI answers `-32603` with a desktop-only message (not `-32601`). |
| B10 | v3.15.1 `subagents/readRuntimeConfig`: sent only when runtime preferences set `subagentRuntimeConfigEnabled` | GPUI never enables it, and answers `-32603` like Desktop without a reader; the CLI keeps its own Markdown profile loading. |
| B8  | `controller/workspaces` + `controller/tasks-index` topics are **host-served** (`packages/desktop/src/host/windowHostControllerService.ts`); the CLI has no dispatch case                                                | Full task index, pin/archive membership and snippets require the existing host boundary. GPUI's loaded-session filtering is not a substitute.                                                                                                                                                     |

## 4. Current implementation slice (2026-10-06)

The action-level audit found that Settings is a missing destination, not an icon-only
omission. The implementation contract and regression cases are in `SETTINGS.md`.
Safety fixes precede preference controls: propagate failed outbound enqueue, scope and
redact RPC status errors, unify bootstrap/data-root resolution, and reject obsolete
connection pumps and restart callbacks. Preferences have one serialized background
writer; Settings navigation remains per window. The backend still owns admission,
subscriptions, revisions, and session configuration.

The settings slice covers navigation, locale, theme, interface font size, supported
shortcuts and committed Memory runtime preferences. Provider/capability settings, upload orchestration, plugin/workflow management,
and host-owned task metadata remain separate audited backlog items. Existing exclusions
in section 1 remain binding. Milestone headings below are historical implementation
checklists, not evidence that every user action is reachable or runtime-verified.

The next composer/activity contract is in `COMPOSER_PARITY.md`: one aggregate
background-work entry, safe child navigation, and per-work cancellation reuse the
existing backend projections and command boundary. Queue reorder and full-item Edit restoration now have reachable
controls with mirrored revision and accepted-delete restoration guards. Local-path
attachments do not require chunk upload. Provider, skill, hook and subagent Settings
management lack current CLI ports and remain integration-blocked, not file-edit substitutes.

## 5. Historical milestone plan

Sizes: **S** ≤ 3 days, **M** ≤ 2 weeks, **L** ≤ 5 weeks, **XL** = needs a spike first.
Each milestone lists **Requirements**, **Acceptance**, and **Landmines**.

### P0: Make what exists trustworthy (historical requirements; current status in §2)

Requirements:

- `cargo fmt` the tree; CI green on `windows-latest`.
- Pass `--surface desktop` (B6) and mirror the B7 env set (read the desktop's
  `buildHostProcessEnv` + `resolveSpawnEnv`; carry only keys whose values the GPUI app
  can actually know).
- Stop the launcher's "self-heal" that copies `zcode-builtin.json` **into the installed
  app's directory**: writing into another product's install is unsafe, and the env var
  already covers it.
- Explicit reverse-RPC table in `backend/events.rs`: each agent→client method is
  _served_, _deliberately failed fast with a reason_, or _raced_ (interactions). No
  silent `-32601` for methods listed in §3. B1/B2 fail fast with "Zai account providers
  are not supported yet; configure an API-key provider" until M11.
- API-key providers are the supported auth path: verify that keys configured in ZCode
  reach the GPUI-spawned agent (same provider config the desktop uses) and document it
  in the README troubleshooting section.
- Live pass of the v1 M1 acceptance script against a real backend; record results here.

Acceptance: CI green; a session opened in both clients gets the same system prompt
(verify via `session/debug`); the reverse-RPC table is covered by golden tests.

Landmines: changing `--surface` changes prompts for _existing_ GPUI sessions mid-flight,
so ship it as one release note. Job-object `.expect()`s in `launcher.rs` can panic the
UI process on exotic Windows setups; convert them to logged fallbacks while in there.

### M4: Composer completion (L) — historical requirements, not a completion claim

Requirements (all wire-backed):

- **Attachments**: `v4/attachment/{begin,chunk,commit,abort}`, chunk ≤ 512 KiB decoded,
  total ≤ 20 MiB (`core.ts:84-98`); `v4/attachment/put` is internal-only and must never
  be called. Sources: file picker, drag-drop (gpui external paths), clipboard image
  paste. Thumbnails via `v4/attachment/previewSource`.
- **Slash commands**: from the `workspace-config` topic's `slashCommands[]`, with
  `workspace/readPresentation` as fallback; popup filtered as you type.
- **Suggestion keys (2026-10-21)**: while the slash/@ popup is open and the composer has focus
  (no IME composition), Up/Down move the highlighted row (wrapping) and Tab takes it
  (desktop `ChatPromptActionMenu` parity). Tab is ely's app-wide `FocusNext` binding and gpui
  runs a binding's action before key listeners, so the composer container captures
  `FocusNext`; without a popup Tab passes through untouched (`composer/autocomplete_keys.rs`).
  The highlight resets when the text changes or a suggestion is taken. Enter still sends.
- **@mentions**: files (local tree + ripgrep), sessions, skills (`skills/referenceCatalog`),
  plugins; inserted as atomic chips; sent as `context_refs` (≤ 1) / text per the desktop
  mention providers (`packages/ui/src/mentions/providers`).
- Remaining V4 commands: `forkAssistant`, `setFollowupMode` + `requestedDelivery:guide`,
  `editQueueItem`, `reorderQueueItem`, `setAssistantFeedback`, `compact`,
  `sendGoalCommand` / `pauseGoal` / `resumeGoal`, `stop{expectedForegroundExecutionId}`,
  `cancelBackgroundWork`.

- **Model/thought selectors** (`composer/menus.rs`): ely `DropdownMenu`/`Menu`
  components render the model menu grouped under provider headings
  (`Menu::group`; grouping in `composer/catalog.rs::group_models` mirrors
  `packages/ui/src/lib/modelSelectionGroups.ts`). Ely floats the panel under the
  trigger and flips it above at the window's bottom edge. The trigger shows
  `Provider/Model` only when the catalog has more than one provider group, else
  the bare model name (desktop `resolveV4ModelTriggerDisplay`). No "manage
  models" footer: gpui has no provider-settings surface to open.

Acceptance: drag a screenshot in, mention a file, run `/compact`, fork from a reply, and
reorder the queue, all matching desktop results on the same session.

Landmines:

- The desktop composer is **Lexical** (`LexicalChatInput.tsx`, 1,531 lines). gpui has no
  rich editor; chips need a custom element in the existing IME-capable `composer/input.rs`
  (UTF-16 marked ranges already handled). Keep the composer a plain string with a
  side-table of chip ranges; never let IME composition split a chip.
- `forkAssistant`, `setAssistantFeedback` are row-targeting: they need `baseLogEpoch`
  (`ROW_TARGETING_COMMANDS`), and stale retry follows §6.

### M5: Shell, settings, i18n, theme (L) — historical requirements, not a completion claim

Requirements:

- **Add project (2026-10-05)**: the Projects section header carries a "+" that
  opens a native directory picker (Electron parity: `WorkspaceSidebar`
  add-project → `PlatformChannels.SelectDirectory` =
  `dialog.showOpenDialog({openDirectory, createDirectory})`). The picked
  folder becomes a project workspace (added, activated, agent spawned) and is
  best-effort persisted to `setting.json` `recentProjects` (front, capped at the
  desktop's 10) and `lastWorkspaceSession` (appended once, uncapped, so it never falls
  off the list) via the serialized preference owner — refused while the desktop runs
  (it rewrites the file whole), in which case the project still exists for the session.
  The same applies to "Remove from sidebar". The desktop re-derives both lists from its
  project table, so a project removed only in GPUI returns once the desktop writes the
  file again if the desktop still has it.
- **Settings**: read `~/.zcode/v2/setting.json` for locale, theme, font size, shortcuts,
  keep-awake, etc. **Writes only when the desktop is not running** (see landmines);
  otherwise show "change this in ZCode desktop".
- **i18n**: generate a Rust table from `packages/ui/src/i18n/locales/{en-US,zh-CN}.ts`
  (~4,980 flat keys, custom `{key}` placeholders, `IntlProvider.tsx`) with a build-time
  script; reuse key names verbatim so translations stay shared. Locale: `system | zh-CN | en-US`.
- **Theme**: Zai Dark + Zai Light + follow-system (`packages/ui/src/styles.css` tokens);
  UI font scaling per DESIGN.md: one `ui_font_size` base (default 14) and the `text-ui-*`
  ladder (+4/+2/0/−1/−2/−4/−5).
- **Shortcuts**: port the 25 commands in `packages/shared/src/shortcutCommands.ts`
  (single source of truth, including defaults); command center (Ctrl+K / Ctrl+Shift+P)
  over the `quickPickCommands.ts` catalog.
- **Sidebar parity** (B8): native task index over all `sessions-index/<ws>` topics plus
  `session/list` without a workspace filter (cross-project). Pin/archive/unread are
  host-owned task meta; locate the desktop's store before deciding read-only vs write.

Acceptance: switch to Chinese and Light theme, scale fonts to 16 px, find a task in
another project via Ctrl+K, and all of it survives restart and matches the desktop.

Landmines:

- **`setting.json` is rewritten whole** from the desktop's in-memory state
  (`settingService.ts:214`, `atomicWriteText`, in-process queue only). A GPUI write while
  the desktop runs is silently clobbered (last writer wins). Detect a running desktop
  (its single-instance lock) before writing.
- i18n source files are TypeScript, not JSON: the extractor must fail the build on
  unparseable entries rather than drop keys.
- Host-owned task meta (pin/archive) may live in the host's task storage worker; if so,
  it is unreachable without the host, so ship read-only or local-only and label it.

### M6: Lifecycle and OS integration (L) — historical requirements, not a completion claim

Requirements: multi-window (one window per workspace/session); single instance with
forward-to-first (named mutex + pipe on Windows); window size/maximized persistence
(**own key/file**, not the desktop's `desktopWindowSize`); task-completion notifications
(suppressed while focused, click focuses the task); tray with show/quit and
close-to-tray on Windows; keep-awake while a turn runs; open-in-editor / file manager;
log export (zip `~/.zcode` logs + panic log); clean quit that kills agents first.

Acceptance: run for a full workday with two windows, background notifications and
tray, with zero orphaned `app-server` processes after quit or crash.

Landmines:

- **`zcode://` scheme ownership**: the installed desktop registers `zcode://` (OAuth,
  payment, share import, `workspace/open`). If GPUI registers it too, deep links reach
  whichever registered last and the desktop's OAuth breaks. Use a distinct scheme
  (`zcode-gpui://`) or none.
- **Coexistence with the desktop on the same workspace**: both spawn their own agent for
  the same workspace key. Verify agent storage tolerates two writers (sessions DB) before
  advertising side-by-side use; otherwise warn when the desktop holds the workspace.
- gpui multi-window + the current single `RootView`/`AppState` design: `AppState` must
  become one shared entity, with per-window view state, before the second window exists.

### M7: Workflows, automations, usage, MCP, subagents (L) — historical requirements, not a completion claim

Requirements:

- **Workflows**: V4 `workflowRuns` snapshot region + `workflowRun.updated/removed` deltas
  (apply order **header → removals → upserts**, `delta.ts:117-137`); queries
  `v4/conversation/workflowRun{s,Events,Artifacts,ArtifactData,ArtifactRead,Workspace,NodeResult}`;
  commands `startSavedWorkflow`, `resumeWorkflowRun`, `amendWorkflowRunSettings`;
  saved workflows via `workflows/{list,get,updateMeta,delete,runs,move}`. Run timeline
  first; graph view later.
- **Automations / off-peak** (B4): superseded requirement. Scheduling remains
  host-owned; current GPUI reverse requests fail fast. Any future integration must
  reuse an existing host scheduler rather than create a second owner (see §7 D3).
- **Usage**: `v4/usage/stats` (legacy `usage/stats` is deprecated); native bar/line/
  heatmap drawing (no chart crate needed).
- **MCP**: `mcp/list` in connect mode with the desktop grouped-row layout (SETTINGS.md
  "MCP Servers page"); editing stays in the desktop (host `mcp-sync` channel).
- **Subagents / background work**: the listed projection/navigation controls exist;
  comprehensive historical browsing and native acceptance are not implied.
  - **V4 Subagents State & Projection**: `Row::Subagent` with `tool_call_id`, `subagent_type`,
    `status`, `summary_text`, `parent_tool_call_id`, `child_session_id`, `work_id`, `backgrounded`,
    and `started_at`. Incremental `row.delta` streaming appends to `summary_text` via `stream_field_mut`.
    `SubagentsState` tracks `revision`, `child_session_ids`, `running`, and `ended_total` from
    `state.updated` patch `subagents`. `join_running_subagents` merges running subagents with
    `backgroundWorks` by `child_session_id` to establish live status and cancellation control.
  - **Transcript Card Pairing**: `transcript/subagent_card.rs` pairs `Row::Subagent` beneath its
    parent `Row::ToolCall` when `tool_call_id == parent_tool_call_id` (matching web/desktop layout),
    and falls back to standalone rendering when unpaired.
  - **Dock Agents Section**: Collapsible Agents section in the Review dock (`agents_section.rs`,
    cards in `agent_card.rs`) displaying running count badge, status dot, model chip, elapsed
    duration, current todo with `completed/total`, one-line summary preview,
    Stop button (sends session command `cancelBackgroundWork { workId }`), and Open button. Displays
    `ended_total` counter when completed subagents exist. Layout follows Desktop
    `SubagentStatusSection`: the title is a 2-line clamped block and elapsed / Stop / Open sit
    on a wrapping row below it, so long titles never push the controls out of the panel.
    Opening a child or returning to the parent leaves the dock open/closed state unchanged.
  - **Child Navigation & Read-Only Gating**: Clicking Open sets `viewing_child: Option<String>` in
    `AppState`, routing `active_conversation()` to the child session while enforcing strict read-only
    mode: suppresses composer input (replaced with read-only banner), turn actions (edit/retry/fork/undo),
    and interaction cards. Top "← Back to {parent_title} ({child_session_id})" bar navigates back to
    parent and cleanly unsubscribes child session via `v4/conversation/unsubscribe`.
  - **Sessions Sidebar Filtering**: Excludes all subagent child session IDs from workspace session lists
    by aggregating `child_owner` and loaded `subagents.childSessionIds`.
  - **Follow-up Scope**: Full on-demand paginated historical drawer via `session/subagents` (for browsing
    ended subagents across past pages) remains an optional follow-up inspection tool; current state
    directly reflects live `state.updated` projection and `ended_total`.

Acceptance: start a saved workflow, watch nodes progress, view an artifact; check this
month's usage; see MCP server status; an agent-created automation either works or fails
with a clear, actionable message.

Landmines:

- **Double scheduling**: if GPUI schedules automations while the desktop also runs, jobs
  fire twice. Pick one owner (D3) and enforce it with the desktop's lock.
- `workflowRunDeltas` capability must only be declared if the host advertised it
  (`clientHello.capabilities` is strict, `transport.ts:78-86`); stdio has no hello today,
  so stay on the legacy 256-node clamp unless verified.
- Closed enums (`backgroundWorkSummary.kind`, `turnHeader.origin`) break the whole frame
  on the TS side; our parser must keep tolerating unknown values (it does; keep a
  golden test for it).

### M8: Plugin store (M) — historical requirements, not a completion claim

Requirements: `plugins/{overview,referenceCatalog,referenceCatalogWithCategory,describe,
install,update,uninstall,setEnabled,configure,resetConfig,restoreBuiltin,validate,
cancelOperation}`, `plugins/marketplace/{add,remove,update}`, progress via the
`plugins/operationProgress` notification. UI and vocabulary strictly per `CONTEXT.md`:
Official Marketplace vs Personal Source, Public/Personal segments, Featured,
Installed Strip, Manage Installed View, Catalog Auto-Refresh (silent, throttled,
official-only) vs Manual Refresh, Restorable Builtin, Orphaned Installed Plugin.

Acceptance: CONTEXT.md lifecycle end to end (add personal source → install → enable →
configure → update → uninstall → restore builtin), with state matching the desktop.

Landmines: plugin management also has host channels (`plugin-management`,
`plugin-sync`); confirm stdio covers persistence or label host-only actions. Restorable
builtins must not auto-reseed. Detail pages render markdown listings (reuse M2).

### M9: Panes long tail (M)

Reachable: per-file stage/unstage/discard, basic branch cycling, terminal scrollback
selection/copy and bounded static raster image preview. The bounded native adapters and
automated evidence are described in FILES.md and TERMINAL.md, not a full M9 completion claim.
Open: safe branch-picker owner integration, git graph, terminal tabs/context menu/persistent
sessions, host-owned file search and plan-detail long tail. PDF/Office remain external-only;
the current React renderers do not establish a native GPUI rendering implementation.

Landmines: Windows ConPTY quirks (startup cursor query handled; verify `Ctrl+C` and
resize under PowerShell 7 and cmd); terminal per window vs per workspace once M6 lands.

### M10: Distribution (M) — historical requirements, not a completion claim

Installer (NSIS or MSIX) + portable zip, icon, version from the repo; self-update via
the desktop manifest feed concept (`ManifestUpdateProvider`, stable/preview channels)
or GitHub releases; crash and log export; macOS/Linux build matrix (Linux CI needs
X11/Wayland dev packages). Webview spike (WebView2/wry) only if a browser pane becomes
a requirement; otherwise "Open in ZCode desktop".

### M11: Zai account / coding-plan auth (XL → spike first, lowest priority) - DEFERRED PER OWNER DECISION

Deferred by the owner (2026-10-03): direct API-key providers do **not** use the
provider-headers port ("普通 API 不进入此端口", `provider-runtime-headers.ts:25`), so
the app works without this. Only Zai account / coding-plan users need it. Until it
lands, P0 makes B1/B2 fail fast with "Zai account providers are not supported yet;
configure an API-key provider".

Requirements:

- **Spike (S)**: decide between
  - **(a) Reuse desktop credentials.** `credentials.json` values are AES-256-GCM with a key
    = SHA-256 of `ZCODE_CREDENTIAL_SECRET` or a host/user-derived string
    (`packages/services/src/credential/providers/credentialCipherProvider.ts:21`).
    Same-user native read is possible. Port the header construction from
    `accountRequestAuthService` (`zcodeAgentService.ts:2245-2296` path) to Rust.
  - **(b) Native login.** OAuth redirect is `zcode://oauth/callback` and needs
    `BIGMODEL_OAUTH_APP_SECRET`, which the desktop gets from its build env, not from the
    repo. That makes native OAuth likely infeasible; API-key login (`apps/zcode-cli`
    `auth-login.ts` key layout) is feasible.
  - Recommended: **(a) for account tokens + (b-API-key) for direct providers**. The
    user logs in once in the desktop app and the GPUI app reads the same store.
- Serve B1 and B2 from the chosen store; handle `interaction/providerRuntimeHeadersCancelled`.
- Token refresh: follow the desktop's refresh path; never log header values.
- Minimal account UI: signed-in identity, provider list, "Sign in via ZCode desktop" when
  no token.

Acceptance: a coding-plan account that works in the desktop chats in GPUI with no extra
steps; revoking the token in the desktop is reflected on the next request.

Landmines:

- **Credential file lock.** `credentials.json` writes use a directory lock
  (`withFileLock`, `packages/shared/src/node/privateFilePersistence.ts:51`). Any GPUI
  write must implement the same lock protocol or corrupt the desktop's store; prefer
  read-only.
- The key derivation is weak by design and slated to move to OS keychain/safeStorage
  (comment in `credentialService.ts`). Isolate it behind one Rust trait so a keychain
  migration is a one-file change.
- Secrets must never reach logs, panic messages or the `%TEMP%` panic log.

## 5. Dependency order

```
P0 ──► M4 (composer) ──► M7 (workflows/automations)
  │
  └──► M5 (shell/i18n/theme) ──► M6 (lifecycle) ──► M10 (distribution) ──► M11 (Zai auth)
                    └──► M8 (plugins)      M9 (panes) runs alongside any of them
```

This is the historical dependency graph, not a statement that CI is currently red.
The first Settings/safety slice is implemented; M4 (composer) remains a daily-use gap. M5 precedes M6 because
multi-window needs the shared-state refactor and persisted settings. M8 and M9 can be
interleaved whenever capacity frees up. M11 is last by owner decision: the owner uses
API-key providers, which work without it.

## 6. Command rules (normative, unchanged)

(`backend/session_cmds.rs`; spec: `zcode-protocol-v4/command.ts`)

- Every session command goes through `send_session_command`, which registers a
  `Pending::Command` so its `CommandAck` is checked.
- CAS commands (`COMMANDS_REQUIRING_BASE_REVISION`) take `baseRevision` from the
  mirrored conversation; row commands (`ROW_TARGETING_COMMANDS`) also need the
  snapshot `logEpoch`. With no snapshot/epoch the action is disabled; the client never
  sends a guessed revision or epoch.
- `stale` → resend once from `revisionAtDecision` with a new commandId.
  `rejected`/`failed`/second `stale`/JSON-RPC error → error banner plus a forced
  resync of the affected topic. The backend snapshot is the only rollback path for
  optimistic UI updates.
- Interactions are answered **only** through V4 `resolveInteraction`; legacy
  `interaction/requestPermission|requestUserInput` stdio requests stay unanswered (the
  backend races them, and a stdio reply loses Allow-always/Full-access). They are
  re-announced every second after a restore, so handling must be idempotent by
  `requestId`.
- Protocol timestamps are CLI-clock Unix ms; never diff them against local time.
- The mirrored `revision` comes only from the snapshot and `state.updated` patches
  (`revision_known` gates CAS until a snapshot arrives). Acks never advance it
  (no `revisionAtDecision + 1`), and `rowsRange.atRevision` never overwrites it. The
  stale retry passes `revisionAtDecision` as an explicit base; a stale ack without it
  fails. `cas_fields` enforces both spec lists centrally, whatever the call site.
- History paging (2026-10-21): snapshot `rows.firstRowId` is the oldest row of the
  **whole** conversation, while `rows.window` is only its tail. "Load earlier messages"
  shows while the oldest loaded row is newer than `firstRowId`, and requests
  `rowsRange {beforeRowId: <oldest loaded row>, limit: 200}`. `hasMore: false` ends
  paging even if hidden rows leave a gap. `row.appended` sets a missing `firstRowId`,
  and `row.removed` from at or before it clears it (shared `apply.ts` parity). Before
  this, GPUI treated `firstRowId` as the window start: the button showed on every
  conversation and asked for rows before row 1, so it never loaded anything.
- Transcript keys (2026-10-21, Desktop `timelineKeyboardScrollIntent`): PageUp and
  PageDown page the transcript (one viewport minus 40 px). Home goes to the top of the
  loaded rows and End returns to the tail. Ctrl+Home and Ctrl+End do the same. PageUp
  and Home stop following new output; PageDown that reaches the tail, and End, resume
  following. Editable controls get the keys first, so the composer keeps Home and End
  for its caret. PageUp and PageDown still reach the transcript from the composer,
  because the composer doesn't use them. Clicking the transcript takes focus from the
  composer, as clicking Desktop's timeline blurs its textarea, so Home and End then
  scroll. This doesn't apply on the phone build. Settings, the dock and the terminal
  keep their keys.
- `resolveInteraction` is idempotent per `interactionId` (`ResolvingInteractions`):
  re-announced cards with an answer in flight stay hidden; a failed answer releases
  the id.
- Reverse RPC: every `automation/*` and `offPeak/*` method (including future ones)
  fails fast with `-32603`. Empty lists or `bound: false` would be false facts, and
  the CLI's binding check must fail closed. Unknown methods get the desktop's
  `-32601 "Unsupported ZCode Protocol request: <method>"`.

### Audit invariants (2026-10-03, extended 2026-10-05)

- **Transport (2026-10-05 audit P0.1/P0.2/P0.3)**: route cursors key on
  `(subscriptionId, logEpoch, seq)`; deltas apply only when
  `fromSeq == cursor.seq` (a one-event gap is a gap); stale/duplicate frames
  (`toSeq <= cursor.seq`) and old-generation frames are dropped without
  touching state; any discontinuity resyncs and returns before the reducer
  runs. The fragment assembler enforces the canonical limits and faults with
  the canonical reason codes; cursor and fragment state are cleared together
  by one registry-based cleanup used by reconnect, unsubscribe, workspace
  unload and connection restart. The stdout pump reads lines through a 2 MiB
  bounded reader (oversized lines are discarded before any large allocation);
  backlog is counted on the producer side with event+byte watermarks
  (32 MiB high / 4 MiB low latch) and a 64 MiB protocol hard bound — overflow
  resets the connection generation and explicitly starts a replacement instead of
  growing memory; the obsolete process's EOF cannot trigger another restart. stderr flows through its own bounded (4 MiB, lossy) queue and can
  never wedge event delivery.
- **Child environment (P0.4)**: the backend is spawned with `env_clear()` and
  an explicit allowlist (system basics + proxy + ZCODE data vars); variables
  like `GLM_BINARY_PATH` or `NODE_OPTIONS` can no longer leak into the agent.
  Custom CA (`NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `SSL_CERT_DIR`) is
  forwarded when present.
- **Client identity (P1.10)**: `client_id` is created once under
  `<data>/v2/gpui-client-id` (parent dirs created as needed) and reused across
  processes; a missing or corrupt file is repaired atomically (tmp + rename
  replace), concurrent creators converge on one winner, and tests cover the
  repair and 4-thread convergence.
- **Sidebar lists (2026-10-05)**: the plain sidebar rows (unlike the desktop's
  virtualized list) render progressively per workspace — the 3 latest
  threads, "Load more" extends, the second click shows everything; the active
  thread stays pinned in the window even when older than it (e.g. opened via
  Ctrl+K in a long list). Project folders toggle open/closed on the header
  chevron (desktop `group-item.tsx handleHeaderClick` parity); expansion and
  load-more steps are pure `RootView` view state, never server facts. New
  projects arrive through the native directory picker or a command-line folder
  argument; the right-click menu removes them.
- **Secrets**: every sink (in-memory log, agent stderr echo, error banners,
  `lastError` text, panic log, exported log bundle) goes through
  `shared::redact::scrub` (credential keys, `Bearer`/`Basic`, `sk-`/`ghp_`/`AKIA`/JWT
  shapes).
- **Settings**: preferences/recent-project writes go through one serialized owner,
  then `persist_change` / `update_settings_checked_at`: read-modify-write of the
  current file, refused while the desktop's Electron single-instance lock
  (`<userData>/lockfile` / `SingletonLock`) or process is present (re-checked
  before the rename), refused for an unparseable file, and unset fields omitted
  (never `null`).
- **Attachments**: the upload params match the strict transport schemas
  (`connectionId`, no workspace fields); a zero-byte file declares zero chunks.
  Local absolute paths remain a valid `ref` for local workspaces.
- **Composer chips**: mentions are atomic `ChipTable` ranges. Every edit and IME
  composition widens to whole chips; caret moves step over them.
- **Transcript**: history pages `splice(0..0, n)` (no reset, so measured heights are
  kept); while following, the tail is re-pinned on every render, so a streaming last
  row stays bottom-aligned.
- **Agents**: kill-on-close job handles are RAII-owned and closed on every path; the
  `taskkill` fallback runs without a console window; `on_app_quit` kills all agents.
- **ConPTY**: the startup `ESC[6n` is reassembled across reads and only intercepted
  in the first 4 KiB, so later program queries reach alacritty.
- **Workspace refs**: every `plugins/*` and `mcp/list` payload sends
  `{ workspacePath, workspaceKey }` (`plugin_payloads::workspace_ref`). The strict
  `zcodeWorkspaceRefSchema` rejected the old path-only ref with `-32602`, so the
  plugin store and MCP list never loaded.
- **Launcher**: order is `ZCODE_GPUI_AGENT_PROGRAM`, then the compiled backend beside
  GPUI, then bun + branch source (only when installed and built). Desktop-exported
  `GLM_BINARY_PATH` / `ZCODE_AGENT_SERVER_COMMAND` are not overrides.
- **Never launch the installed desktop app (2026-10-30).** GPUI used to fall back to
  `ZCode.exe` for the agent and the Services Host. Its agents also started plugin MCP servers
  through `ZCode.exe` whenever Desktop had last rewritten the shared plugin cache. All three
  paths are gone. The agent rebinds plugin-host MCP servers to its own runtime when they
  load. The desktop install is only ever read from (ripgrep, Built-in Provider release).
  `launcher_tests::installed_desktop_app_is_never_a_candidate` guards the launcher.

## 7. Decisions needed from the owner

| ID  | Decision                                                                                                                        | Recommendation                                                                                                                                                            |
| --- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | Is GPUI a **replacement** for the desktop or a **companion** that runs alongside it?                                            | Companion first (read desktop settings, never write while it runs). It drives D3–D4.                                                                                      |
| D2  | ~~Auth route~~ **Decided 2026-10-03**: API-key providers only; Zai account auth deferred to M11 (route (a) vs (b) chosen then). | —                                                                                                                                                                         |
| D3  | Who schedules automations/off-peak when both apps are installed?                                                                | Current boundary: scheduling stays host-owned; GPUI fails these reverse RPCs fast and never starts a local scheduler. Future integration needs a separate owner decision. |
| D4  | Separate deep-link scheme?                                                                                                      | Yes, `zcode-gpui://` or none.                                                                                                                                             |
| D5  | Confirm the §1 out-of-scope list.                                                                                               | As listed.                                                                                                                                                                |

## 8. Cross-cutting

| Stream               | Content                                                                                                                                                                                          |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Protocol drift       | After each upstream pull: diff `packages/shared/src/zcode-protocol*`, re-run goldens; add a golden per new reverse-RPC method. Capture real traffic for new regions (workflowRuns, attachments). |
| Reverse-RPC registry | One table (P0) is the source of truth for served / failed-fast / raced methods; CI test asserts every method in `zcodeProtocolMethods` that the agent can send to the client is listed.          |
| Performance          | Per milestone: 10k-row session at full frame rate, 8 projects, idle memory budget (frontend ≤ 80 MB, one agent ≈ 120 MB).                                                                        |
| Security             | No credentials or provider headers in logs or the panic log; credential access behind one trait.                                                                                                 |
| Governance           | ≤ 400 lines/file, new modules land in the owning slice (see README "Source Layout"), this file updated before behavior.                                                                          |

## 9. Risks

1. **Model catalog harvest** via legacy `session/create` + `deleteSession` remains the
   only catalog source (no V4 query); golden-tested, but upstream can break it.
2. **Host-only surfaces grow upstream.** New desktop features often land as host
   services, not agent methods. Track `packages/shared/src/channels.ts` in the drift check.
3. **Credential format change** (keychain migration) would break M11a overnight; the
   trait boundary limits the blast radius.
4. **Local git/terminal/files** deviate from "all logic in the backend", but the desktop
   host is itself a local implementation (node-pty, git service), so no new remote
   semantics are introduced (v1 decision, kept).
5. **Process overhead**: solved pre-M0 (lazy spawn, idle unload, job-object kill);
   startup ~267 MB vs ~1 GB with eager spawn.
