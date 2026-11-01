# Settings and safety slice

Status, 2026-10-06: native navigation, complete shortcut vectors, configured-model inspection
and opt-in local Subagents management are implemented using the unchanged full Services Host.
The rejected inventory has been rebuilt with grouped horizontal Ely rows and inline overrides.
**The redesign has not passed actual native visual acceptance.** Current verification and
remaining GUI/restart cases are in SETTINGS_ACCEPTANCE.md. This spec does not expand PARITY.md exclusions.

## Approved local management extension (2026-10-06)

Subagents gains explicit real-local activation through the unchanged full Services Host.
It cannot start from render before consent. Native preference admission suspends and drains
before the Host starts; other sections/Close manager/idle initiate quiescent stop, observed
process exit and strict reload/rebase before persistence resumes. Deferred recent projects
stay on the existing bounded preference owner. Isolated preferences remain read-only.
Unsupported native appearance fields are not sent through ISettingService; effective theme
and font remain stable in memory if canonical normalization omits them. No stale full-file
restoration or cross-process ownership guarantee is added. See SERVICES_RPC.md.

Settings is an exclusive per-window destination: its navigation replaces the workspace
sidebar while open. Projects, Tasks, New task, workspace Search and runtime status are not
rendered alongside it. Back to workspace lives in the Settings navigation and restores the
same workspace/session, draft, sidebar expansion/limits, dock and terminal. Active streams,
permissions and Host cleanup remain owned by their existing state owners. No workspace is
removed or unloaded by this render transition. At narrow widths the Settings navigation
becomes a bounded top region, still without the workspace sidebar.

One selected-section heading supplies the page title. Subagents uses compact scope/count/right-search,
custom-header Refresh/New, rounded shared lists, dense horizontal Ely icon/badge rows,
aligned switch/delete and plugin/builtin inline selectors. Ordinary Subagents content does
not include the full-width advanced settings-file button. Responsive/focus/locale/theme
native acceptance precedes any visual-complete claim. Earlier counts below are historical.

## Product behavior

- The sidebar gear, command-center Settings action, and platform Settings shortcut
  open one per-window destination. General is the initial section. Reopening reuses
  the page. Back/Escape restore workspace focus without changing workspace/session,
  draft, transcript state, dock, or terminal. Session streaming and lifecycle work
  continue while Settings is displayed.
- General has one language selector. English is the default when no valid language
  preference is saved; an explicit English, Chinese or System selection remains respected.
  UI labels render only the selected language, never concatenated English/Chinese text.
  The existing serialized preference owner persists selection and projects the same effective
  locale to native labels and Ely components after a successful save. No second locale writer
  is added. Selecting language while preference persistence is suspended/read-only remains
  refused; debug acceptance locale overrides are non-persisting and isolated-only.
  One resolver reads `localePreference`, falling back to legacy `locale` only when the
  preference is absent; unset/invalid values resolve to English without saving a default.
  System remains an explicit choice. English choices read English / Chinese (Simplified) /
  System; Chinese choices use their Chinese labels. Ely's existing I18n global is a derived
  projection with its built-in English/Chinese component catalog, not another preference owner.
  Changing language does not rewrite user or plugin content. Synthetic default acceptance
  data uses monolingual English labels/content. The native acceptance harness may use
  `--locale preference` to omit the non-persisting locale override and verify the scratch
  settings' actual default/readback; this does not make isolated preferences writable.
- Appearance exposes System/dark/light theme and UI font size (12–20 px). Theme preference
  and effective OS appearance are distinct. UI typography scales without scaling icons,
  geometry, code, or terminal.
- Shortcuts show only commands implemented in this client. Overrides use the shared
  command IDs and binding syntax; unsupported IDs are preserved but not advertised.
  Invalid/conflicting overrides fail validation. Empty overrides disable a command;
  reset removes the override. One dispatch path prevents duplicate execution.
- Settings-file opening is an explicitly named advanced action, not Settings navigation.
  Host-owned or unavailable controls are not shown as functioning toggles.
- Sidebar Search and command-center shortcuts use the same focus transition. Closing
  the overlay restores Settings focus or composer focus, including pointer dismissal.
  Sidebar workspace/session/new-task and Add Project actions return to the workspace;
  cancelling the folder picker leaves the current workspace and draft unchanged.
- A focused shortcut recorder consumes recording/clear/Escape events before application
  navigation. After recording or cancellation, Settings retains a usable focus target.

## Confirmed remaining Settings owner ports (2026-10-06)

Skills enable/delete and Commands CRUD/toggle remain unavailable: current React uses
service-owned writes, while CLI stdio has no matching public management command. Subagents
uses the separate existing Services channel after explicit local activation; CLI stdio alone
still has no profile-management API. Composer reference catalogs and
runtime child navigation are not configuration owners. Hooks has a real
`workspace/hooks/trustGrant` write, but Settings still lacks a CLI-owned canonical hook
snapshot read supplying current `bundleDigest`/`hookDeclarationDigest`. Trust must not
be exposed using invented digests or a second frontend config parser. These are explicit
integration blockers, not completed parity or permission to edit configuration directly.

## Native-only remaining configuration boundary (2026-10-06)

The existing TypeScript backend remains unchanged by owner requirement. The proposed new
configuration worker and service/repository edits were withdrawn. SUBAGENTS_SETTINGS.md
records opt-in local capability, preference suspension and observed Host exit/reload. The
current CLI has child-session queries, not profile CRUD or model-override management.
Canonical controls use the unchanged full Services Host, in disposable acceptance or
explicitly activated local management. No new protocol route or frontend-owned
configuration writer is authorized. Models, Skills, Commands, Hooks and MCP
configuration have the same owner-access prerequisite where existing native ports are
missing. Browser execution remains excluded.

The unchanged-interface assessment in SUBAGENTS_SETTINGS.md confirms that Desktop's
service port is private Electron IPC, not an external native attachment endpoint. The
existing server stdio entry exposes the service but constructs a full Host with eager
provider startup and conditional shared-storage migration. Opt-in local management now
accepts those disclosed existing-Host behaviors; it is not lossless, migration-free,
concurrent-safe or standalone packaged parity. No native profile writer is claimed.

## Approved native-only implementation (2026-10-06)

SERVICES_RPC.md defines a Rust client for the existing unchanged full stdio Host. Disposable
--isolated-settings uses owned roots and read-only native preferences. Ordinary Subagents
navigation starts no Host until activation consent; management suspends native preference
writes and returns ownership only after verified process-tree exit and fresh reload.
The CLI conversation connection stays unchanged; no backend, protocol or service edits.

Settings navigation adds grouped sections, typed direct-section intents, per-window
remembered section, bounded scrolling and smaller-window layout. The native `--settings-section <section>` launch intent opens a known section through the same typed path; invalid section names are rejected before launch. This launch intent does not bypass local-management activation. Configured Models is
a read-only view of existing workspace model metadata, not provider configuration or
Subagents defaults. Shortcuts search and edit the complete binding vector without
clobbering another binding; clear/reset retain current semantics. No unsupported command
is advertised. All controls use Ely and existing theme/typography/localization adapters.

Native interaction cases cover duplicate/direct entry, Back/Escape during streaming,
per-window isolation, narrow layout, models without changing session selection, shortcut
multi-binding edits/reset/conflicts, and refusal of preference writes in isolation.

## Owners and event order

```text
gear / shortcut / command → RootView navigation → Settings view draft
Settings mutation / recent project → process preference owner
    → validated serialized background read–modify–write
    → committed snapshot → effective theme / locale / typography / shortcuts
failure → unchanged committed snapshot + visible error

session command → backend admission → ACK + authoritative stream
    explicit rejected / stale / failed → original-context draft recovery
    accepted / duplicate / noop → no replay; malformed → visible uncertain outcome
    create ACK → select only when original navigation generation is still current
connection spawn → generation → pump / flow / delayed restart
    → apply only if generation is still current
```

Application preferences use one controller and one disk mutation path. Unknown JSON
fields round-trip. Desktop-running checks remain before writing and before replacement.
A corrupt/unreadable file is never overwritten. Temporary writes are unique and cleaned
on failure. IO runs off the UI thread. A failed write cannot appear as a committed
preference. Navigation is not backend state; pending preference mutations are not an
accepted task queue.

Bootstrap settings remain under the resolved user home `.zcode/v2/setting.json`.
Application data uses `persisted dataBaseDir > ZCODE_DATA_BASE_DIR > default home`,
then appends `.zcode`, matching services/paths.ts. Discovery, install identity and child
backend receive the same resolved base. No data migration/deletion is implicit.

## Safety fixes and acceptance

| Case                     | Setup/action                                                  | Assertions                                                                    |
| ------------------------ | ------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Send enqueue failure     | Connected handle whose stdin receiver closes before send      | Returns failure; no orphan pending entry; composer text/attachments restored  |
| Background RPC error     | Workspace B fails while A is active; sentinel secret in error | Only B status changes; banners/status/logs contain no sentinel                |
| Custom roots             | Persisted root, env root, conflicting home variables          | Bootstrap location stays separate; identity/workspace/backend share data root |
| Old connection work      | Unload/respawn then deliver old events and delayed restart    | No state changes, flow signal, or restart on replacement generation           |
| Settings entries         | Gear, command, configured shortcut; repeat                    | One destination; General initial; no duplicate dispatch                       |
| Back during running turn | Draft, dock and terminal exist; open/close Settings           | Same context/draft; stream continues; composer regains focus                  |
| Preference commit        | Change locale/theme/font; reopen/restart                      | Committed value applied and persisted; System follows effective OS theme      |
| Write refusal            | Desktop starts, corrupt JSON, unwritable path                 | Old snapshot/file preserved; visible failure; no leftover temporary file      |
| Serialization            | Overlapping preference and recent-project mutations           | Both survive in order; unrelated/unknown fields preserved                     |
| Shortcut override        | Valid, invalid, duplicate, empty and reset                    | Correct live dispatch; validation error; disabled/reset semantics             |
| Platform PTY             | Platform shell prints computed sentinel                       | Actual output verified, not input echo; child cleaned up                      |

Existing settings/transport tests are extended; new pure owner/generation tests and
native interaction scenarios are required. Test windows isolate preference files and
skip native geometry persistence, keep-awake, and notification side effects. GUI/real-backend
execution is reported separately from unit coverage. No historical test count establishes
current success.

## Earlier slice validation (2026-10-06, Windows checkout)

The following counts describe the earlier Settings/safety slice, not the final native-only
Subagents implementation. SETTINGS_ACCEPTANCE.md records current gate results, real Host
and same-server runtime evidence, the rejected appearance and remaining production gates.

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: passed.
  Cargo reports the ReFS incremental-cache hard-link fallback, not a source lint.
- `cargo test`: 429 unit tests, 1 PTY smoke, 8 reverse-RPC integration tests,
  and 1 source-limit integration test passed. All Rust source/test files remain ≤400 lines.
- Eight asset-capable GPUI Settings tests cover keyboard/QuickPick/action entry,
  measured-bound gear/section/Back pointer events, recorder focus and live shortcut
  dispatch, serialized preferences/refusal, new-owner hydration, and a real event-pump
  frame carrying running conversation/permission state while Settings remains open.
  Four additional Memory regressions cover pointer persistence/re-hydration, pending
  versus committed origin-only runtime replies, refused/corrupt writes and typed defaults.
  MCP adds strict authorization/status projection, unsafe URL refusal, refresh/reconnect
  receipt isolation, generic secret-safe errors and a measured Open/Refresh pointer test.
  Seven delivery tests additionally cover one-shot keyboard/pointer routing, platform
  modifiers, IME suppression, first-input/local-action compatibility, projection replacement,
  attachment ownership on refused/failed enqueue and rejected admission with newer drafts.
  Six held-queue tests cover measured Clear/Keep/Cancel, reviewed-ID payloads, new-goal
  objective routing without modifying /goal resume or /compact, current
  owner/config/draft/queue validation, stale ACK re-confirmation and no optimistic removal.
  Workflow successor navigation adds projected identity/ordering/owner tests and measured
  Open successor/All runs/Open run controls; the existing Apply test now checks integrated
  ACK-then-stream following without replacing the parent draft.
  Eight slash-catalog regressions cover strict empty/invalid decoding, draft/session ownership,
  no fallback, reconnect/late replies, attachment-preserving selection, stale Refresh,
  raw firstInput/sendText recovery and measured `/init` selection followed by keyboard Send.
  Seven `/plan` regressions additionally cover local-only preparation, model/mode selector
  overlays, nonrecursive task routing, normalized firstInput/config, attachment/chip/IME
  refusal, modified delivery, held confirmation and failed/rejected recovery. The same
  interaction fixture exercises pointer task submission and bare-command keyboard handling.
  A further measured menu/keyboard test verifies draft thinking-level edits use that override.
  Nine image-upload regressions cover metadata/chunk/count/staged-byte bounds, strict IDs/progress/refs,
  generic errors, failed enqueue, explicit Retry/Cancel, stale owner/replacement/reconnect,
  clipboard keyboard and pointer controls, and format-correct owned new-draft temp paths.
  Twelve workflow-artifact regressions cover strict safe-field/version decoding, correct
  parent/run queries, query/pending capacity, empty/unavailable/error states, failed enqueue,
  stale selection/parent/connection/run refusal and measured Open/Refresh/All runs controls.
  They also cover exact-version bounded Markdown reads, strict complete base64/MIME/UTF-8,
  passive links, unique content-selection tokens, View/Retry/Close, and late-byte retirement.
  Seven Files regressions cover six raster formats, one-frame animated GIF handling,
  strict input/dimension/pixel bounds, live localized safe errors, bounded fixture IO,
  measured image/text/image selection with actual background completion and render-atlas
  upload, and same-path identity/workspace-reset stale tree/preview refusal.
  Four terminal regressions add shared cell-metric/viewport geometry, wrapped/wide/combining
  and scrollback copy, selected ANSI cell styling, measured drag/keyboard clipboard readback,
  Ctrl+C ETX without selection, release-outside and workspace/spawn retirement.
  These use a no-op text system, not native glyph/render acceptance.
- The isolated CLI backend smoke passed with a loopback mock API-key provider, actual
  assistant frames in `desktop-continuous` and `web-remote-replayable`, and a new
  `recovery` snapshot at `completedSuccess`. It is not a full GPUI executable smoke.
  It also verifies saved launch completion/history identity, directly published markdown
  deliverable metadata, complete authorized Markdown bytes, cross-parent read refusal
  and unknown-run empty lists, definition inspection,
  metadata replacement (including `default:null`), global-to-project move, destination
  refusal and deletion of disposable harness-created fixtures. Plugin smoke also describes
  a disposable local source and verifies typed scoped configuration, secret readback
  omission/preservation, explicit clear and scope-specific reset against scratch files.
  It verifies idle startNow admission identity, a deterministically held mock response
  with actual busy queue admission, missing/stale held disposition refusal and Keep/Clear
  results from fresh strict snapshots. It accepts enabled Memory replies for both reverse
  request scopes and verifies the current status-only MCP query schema on an empty isolated configuration. No real OAuth
  browser or memory tool is exercised. The smoke also validates real draft/session slash
  catalogs, custom-command discovery and disable-noninteractive filtering, and observes
  the CLI's `/init` expansion (including additional notes) in the loopback request. The
  mock returns text only; no instruction-file write or real model tool is exercised.
  A queued disposable input also retains `mode:edit` and `planEnabled:true` in a fresh
  strict CLI snapshot. This proves frozen admission intent, not plan execution; that
  scratch queue item is cleared without running its task. A real scratch image transaction
  verifies bounded begin/chunk/commit, identical chunk/commit retries, abort refusal and
  artifact resolution into a vision-capable loopback model request. The PNG fixture has
  valid chunk checksums; request bodies are checked in memory, never logged. The standalone
  harness passes strict TypeScript checking and targeted lint.
- Literal `pnpm typecheck` / `pnpm lint` are unavailable (`pnpm` not found). Verified
  `bun run typecheck` passed; `bun run lint` passed with 71 warnings and 0 errors.
  Freshness and Bun architecture checks passed with 0 violations. GPUI is unmanaged
  by that TypeScript architecture policy, so Rust ownership was reviewed separately.
- Targeted formatting passed for README, PARITY, SETTINGS and the smoke script.
  Repository-wide `bun run fmt:check` still fails on 2,851 other files; no unrelated
  whole-repository formatting was applied. Git whitespace checking passes with CRLF
  recognized as a line ending. Cargo lock changes are the GPUI test-support dependency
  graph (including proptest/backtrace), plus a direct declaration of already-resolved
  url 2.5.8 for strict OAuth URL parsing and image 0.25.10 for bounded static preview,
  not a new production framework pin.

Later isolated Windows Subagents screenshots and pointer/typing/create evidence are
recorded in SETTINGS_ACCEPTANCE.md. The appearance was rejected; screenshots are not
visual acceptance. Actual executable persistence restart, full GPUI-to-real-backend
rendered conversation, native folder-picker cancellation and macOS/Linux execution
remain evidence gaps, not passing scenarios.

## Workspace-backed inspection sections

Plugins, MCP Servers and Usage become Settings destinations by reusing their existing
panes and existing stdio queries, not by introducing preference-file writes. Plugins
Settings enters Manage Installed; the marketplace remains the separate workspace dock.
MCP shows live status but cannot edit configuration until the existing owner-management
operations are wired (see "MCP Servers page" below). Usage
retains its backend-supported 7d/30d/all ranges. Each section shows initial loading,
empty success, error and explicit refresh; unavailable/disconnected workspaces say so.

`WorkspaceHandle` is the sole owner of connection-local query results, loading and
sanitized errors. Usage caches are keyed by range; its selected range is workspace
view state and late responses for an older range cannot select it. Views read only
the active handle. Identical pending reads are suppressed per resource. Connection
invalidation clears results. A response captures its originating handle through the
pending registry; plugin mutations refresh that same handle, never whichever workspace
became active. Error diagnostics inside a successful plugin RPC are failures, not
completion evidence. No credentials or raw diagnostics are persisted as preferences.

```text
Settings/dock entry → existing scoped query → workspace pending registry
                   → originating query result/cache → active-handle pane only
plugin mutation → backend result + diagnostics → originating overview refresh
```

Tests cover A/B late responses, range switching, duplicate requests, reconnect clearing,
errors, successful empty results and measured-pointer section navigation/Back. The
native/backend evidence boundary remains the same as the first slice.

## Memory runtime preference

Memory is a Settings section with one application preference `memoryEnabled`, default
false. The process-global serialized PreferenceOwner persists it to the existing
bootstrap `setting.json`, sharing corruption/coexistence/atomic-write safeguards. The
UI uses the committed value, saving/error state and an Ely switch; failed writes never
change runtime replies. Unknown settings survive. This is not a new CLI write method,
a frontend memory store, or an autoload/session-specific preference.

Every `session/requestRuntimePreferences` received on an originating stdio connection
reads the current committed owner and includes that boolean in its strict response.
False disables memory in CLI runtime materialization; true leaves CLI `features.memory`
and `memory.use` policy intact, not a promise that memory is operational without backend
configuration. Materialization freezes preferences at create/resume; a changed setting
applies to newly materialized or resumed runtimes, not the already active runtime.
Existing search/AskUserQuestion response values are unchanged in this bounded slice.

```text
Memory switch → serialized preference owner → background guarded persistence
              → committed snapshot → next reverse runtime-preference request
              → originating stdio connection → CLI create/resume materialization
failed persistence → unchanged snapshot + runtime response, visible error
```

Acceptance: Memory section/switch is pointer reachable; setting round-trips and reloads;
missing/false remains disabled; true is returned only after a successful commit. Tests
exercise both reverse replies through the actual event handler, refusal keeping false,
unknown-key preservation and parent draft/focus. No active-session restart is fabricated.
The disposable CLI smoke validates current request/result schemas and accepts a true
Memory reply for real runtime materialization and user execution. This protocol evidence
is separate from GPUI pointer/owner tests and is not memory-tool execution evidence.
Native visual/executable restart and actual memory-tool execution remain evidence gaps.

## GPUI-only preferences file

`gpuiVoice`, `gpuiNoiseFree`, `gpuiCodeSearch`, `gpuiLspDiagnostics` and `gpuiAutoCompact` are stored in
`~/.zcode/v2/gpui_settings.json`, next to `setting.json`, never in `setting.json` itself. The
Electron desktop rewrites `setting.json` whole from its own typed copy and drops keys it doesn't
model: on 2026-10-21 an Electron start erased `gpuiVoice`, so voice was off after the next GPUI
restart. `shared/settings.rs` is the only reader and writer of both files, behind the same
PreferenceOwner queue:

- Reads overlay `gpui_settings.json` on `setting.json`. A field missing from the GPUI file falls
  back to a legacy value in `setting.json`, so values saved by older builds still load.
- A change that only touches GPUI-only fields writes only `gpui_settings.json`. It skips the
  desktop-running lock, because Electron never touches that file.
- A change to `setting.json` still needs the desktop lock. It drops the legacy GPUI keys from
  `setting.json`, and writes `gpui_settings.json` first when that file doesn't exist yet, so
  legacy values move over instead of vanishing. A refused change writes neither file.
- An unparseable `gpui_settings.json` is never overwritten; the write fails like an unparseable
  `setting.json`.

Acceptance tests (`shared/settings_tests.rs`): values survive Electron rewriting `setting.json`;
GPUI-only changes save while the desktop runs and leave `setting.json` byte-identical; a refused
mixed change writes neither file; legacy values load and move on the next shared write; the GPUI
file wins over stale legacy values; an unparseable GPUI file is kept.

## Background auto-compaction (per model, GPUI only)

GPUI can compact a conversation on its own after it has sat idle for a while, so the next message
doesn't pay for a huge context. It is off by default and set per model.

Setting: `gpuiAutoCompact` in `gpui_settings.json`, a map from `"provider/model"` (the composer's
model value without the `$thought` suffix) to `{ "enabled": bool, "minutes": u32 }`. A missing entry
means off with 50 minutes. Minutes are clamped to 5..=1440 when saved and when read. Writes go
through the PreferenceOwner queue (`PreferenceChange::AutoCompact(key, entry)`); the entry for one
model is replaced whole, other models' entries are kept.

Controls (both edit the same entry):

- Composer: a Timer icon button next to the noise-free toggle. It acts on the composer's currently
  selected model: Subtle when on, Ghost when off; the tooltip names that model's minutes, e.g.
  "Auto-compact after 50 min idle: on". Clicking flips `enabled` and keeps `minutes`.
- Settings › Models: each model row of a provider has a Timer switch and a small minutes field
  (digits only; commits on Enter or blur; empty or unchanged input commits nothing).

Trigger rules. The owner is `AppState`; the 30 s maintenance tick runs the check after the
release sweep. Each mirrored `ConversationState` carries:

- `idle_since`: set when `control.phase` is seen not active (not `running`/`prewarming`), cleared
  when it becomes active.
- `auto_compacted`: set when an auto-compaction starts; cleared when a new run starts (phase becomes
  active while no auto-compaction is in flight). So it fires once per idle stretch.
- `auto_compact`: the in-flight auto-compaction, if any (`Resuming` → `Sent` → `Running` →
  `Settling`).
- `compacted_through`: the last row id when the last auto-compaction ended.

A conversation is eligible when its model's entry is enabled, `idle_since` is at least `minutes`
old, `auto_compacted` is false, `usage.contextWindow.usedTokens` ≥ 20 000, it has no live work
(pending interactions, queued input, running activity), a row newer than `compacted_through`
exists, and it is not a subagent child. Only conversations GPUI has mirrored in this run are
considered: a session never opened since GPUI started has no `idle_since`.

The compaction runs as its own CLI turn, so the phase goes active while it runs, and its
terminal marker can arrive before the turn's last active `control` frame. The flight therefore
stays in flight (`Settling`) until the phase is next seen idle; ending it at the marker let that
trailing active frame reset `auto_compacted`, and the session was compacted again every
`minutes` (2026-11-01: eight empty compactions overnight). The row check is the second guard:
with nothing appended since the last compaction there is nothing to compact.

A finished turn with no visible rows (no user input, text, thinking or tool rows; GPUI draws no
compact marker) and no file changes renders no turn header, so compactions leave no bare
"Worked for 0s" divider. Running and failed turns keep their header.

Its completion signal is the compact `timelineMarker` row: the CLI tags it with
`sourceCommandId` = the `commandId` of our `compact` envelope (`product-projection.ts`
`onCompactLifecycle`), and every accepted manual compact ends in exactly one terminal status
(`success`, `noop`, `failed`, `cancelled`). Core `executeManualCompact` guarantees this through
`settleManualCompactTimeline`, which writes `CompactFailed` when setup throws before or right
after `CompactStarted` (2026-10-31). GPUI has no timeout for a running compaction.

```text
maintenance tick (AppState)
  └─ eligible? → auto_compacted = true, auto_compact = Resuming
       ├─ agent not running → ensure_spawned (once; agent gone again → give up, warn)
       ├─ agent ready, not subscribed → v4/conversation/subscribe (cold resume in CLI gateway)
       │     subscribe failed (no pending, still unsubscribed) → give up, warn
       └─ subscribed → v4/command compact {commandId} (Pending::AutoCompact) → Sent{commandId}
CLI ack ok → Running{commandId} | ack error/rejected → auto_compact = None, warn
compact marker row (delta or resync snapshot) with sourceCommandId == commandId and a terminal
  status → phase active: Settling | phase idle: auto_compact = None, compacted_through = last row;
  the next tick logs "finished (<status>)" (may precede the ack)
Settling and control phase idle → auto_compact = None, compacted_through = last row
Settling and unsubscribed → auto_compact = None (already finished)
Sent and the pending ack vanished (agent connection lost) → give up, warn
Running and the conversation unsubscribed (agent exited) → give up, warn
release sweep / reaper: auto_compact.is_some() counts as live work, so neither drops it mid-flight
```

The compact is sent as a `compact` envelope (empty payload, no CAS) and never retried; a
failure is written to the GPUI log and waits for new activity. Failures don't raise the error
banner because the user didn't ask for this compaction.

Acceptance tests: idle-state transitions (phase idle → `idle_since`, active → cleared and
`auto_compacted` reset, active during an in-flight compaction keeps the flag); eligibility (disabled,
below minutes, below the token floor, already compacted, running, live work); the in-flight steps
(ack; only our terminal marker ends the flight, also before the ack or from a snapshot; no
timeout across many ticks; the compaction turn's own trailing active frame does not re-arm it;
not due again until a row is appended); empty finished turns hide their header; core: setup failures settle with one `CompactFailed`; the setting round-trips through `gpui_settings.json` with
clamping. Live behaviour against a real CLI is a manual check.

## Voice (desktop only)

Settings › Voice sits under Agent capabilities after Browser Use and is hidden on the phone build,
like Browser Use. Its values live in one GPUI-only `gpuiVoice` object in `gpui_settings.json`
(`enabled`, `voice`, `speed`, `volume`, `onlyWhenUnfocused`; defaults off, `af_sky`, 1.0, 0.8,
off), written through the same PreferenceOwner and validated before the write. The
runtime-preference reply carries `voiceEnabled: true` only when the desktop has a voice service
and the committed setting is on, so, like Memory, turning it on applies to newly materialized or
resumed sessions. Turning it off applies at once: Speak calls get `disabled` and current speech
stops. The Test voice button and the engine status line read the voice service, not the
preference. Product rules, protocol and the sidecar are specified in [VOICE.md](VOICE.md).

Acceptance tests: grouped navigation includes Voice and hides it on the phone; the pane renders
on desktop; `gpuiVoice` round-trips with partial-object defaults; the speak handler answers
`disabled`, `unfocusedOnly` and `unavailable` and never sends a blank line to the engine; the
runtime-preference reply includes `voiceEnabled`. Real speech through the GPUI executable is a
manual check.

## MCP Servers page

Layout follows desktop `McpSettingsSection` / `McpServerList`. The page scrolls with the
Settings panel like other sections (no inner full-height panel, no "Open settings file"
footer, which opens the unrelated application `setting.json`):

- An "Installed" group: header with item count and a refresh icon button, then one surface
  card whose rows hold the user/workspace servers. Rows needing attention (failed status or a
  pending authorization URL) sort first; the rest keep the CLI's name order.
- One group per plugin for `plugin:<plugin>:<server>` names, titled with the canonical plugin
  name, rows named by the server part.
- Row: server icon tile with a status dot in its corner (connected green, connecting/pending
  authorization yellow, failed red, otherwise subtlest; tooltip gives the localized reason),
  name, a line `<transport> · <n> tools` (tool count only when connected), the localized
  `settings.mcp.failure.<failureKind>` message for failed rows, and Open authorization.
- Status mapping matches desktop `mcpStoreStatusList`: `failed` is an error, `disabled` and
  `untrusted` show as unknown. Raw status strings and `updatedAt` are not displayed.
- No servers: dashed card with `settings.mcp.emptyTitle` / `emptyDescription`.

Not ported (no GPUI owner for `~/.zcode/cli/config.json` `mcp.servers` yet): enable switch,
New/Edit/Delete form, Import, scope filter and search. Rows have no click-to-edit.

`mcp/list` uses `mode:connect`, as desktop does for page load and Refresh: the CLI connects
and revalidates the configured servers on the process-level `protocol-settings` lease and
reports real status and tool counts. `mode:status` only reads servers that lease has
already connected, so it reported every server as `disconnected` with 0 tools even while
agent sessions (separate leases) used them. GPUI sends no `mcpServers`, so the CLI uses its
own config plus plugin servers, the same set GPUI sessions use. While a refresh is loading
the previous list stays visible.

Acceptance (`app/mcp_pane_tests.rs`, `shared/mcp_tests.rs`): the request is `mode:connect`
without `mcpServers`; the projection keeps a validated `failureKind`; status mapping, attention
ordering and plugin grouping match desktop; MCP translation keys resolve; Settings › MCP
renders grouped rows and the refresh target. Real server connection is a manual check.

## Pending MCP OAuth authorization

MCP Settings exposes Open authorization only when the current CLI status projection
contains an OAuth authorization-code URL. Connect-mode `mcp/list` returns early with the
pending authorization snapshot while the connection continues in the background; the
frontend does not poll. CLI MCP runtime owns the flow. The originating connection-local query is the only owner
of status/authorization facts; the frontend owns only this user-initiated open action.

The typed projection validates the current result schema. Authorization accepts only
absolute HTTP/HTTPS URLs with a host, no userinfo and no raw control characters. Unsafe
or malformed results fail closed with generic feedback. URLs are short-lived sensitive
values: never displayed verbatim, serialized to preferences, or included in Debug/logs.
MCP RPC/server errors use generic feedback because arbitrary URL tokens may be echoed.
Open captures workspace, connection generation, server and authorization receipt; the
click revalidates the current ready origin, active workspace and nonloading/error-free
query. Refresh, replacement authorization and reconnect invalidate old callbacks. Native
URL opening uses the app's secret-safe OS adapter off the UI thread, not a shell command
or browser backend. The pinned GPUI opener can log a full URL on launch failure, so it is
not used for sensitive authorization in production. The adapter reuses ShellExecuteW on
Windows and directly invokes open/xdg-open elsewhere with null child IO; errors are generic.
Query invalidation also retires pending MCP/Usage reads so late replies cannot recreate
old connection facts. A user click commits dispatch; changing workspace afterward does
not cancel native opening, but late launch errors cannot affect a replacement origin.

```text
MCP Settings → connect-mode query → origin pending registry
                 → strict status/authorization projection → user Open authorization
                 → current owner/generation/receipt check → native external URL opener
refresh/reconnect/owner switch → old receipt cannot open a URL
```

Acceptance: measured pointer action, absent/unsafe URL has no action, exact connect-mode
request, origin-isolated late reply, reconnect invalidation, stale callback refusal,
refresh removing authorization and secret-free errors/debug output. Tests must intercept
the platform opener; no real OAuth URL or browser launch is used. Opening a page does not
prove OAuth completion. Native browser launch, real authorization, and MCP configuration
CRUD remain separate evidence/port gaps; no fake credential/configuration store is added.

## Remaining audited backlog

Plugins Manage Installed, MCP inspection and workspace/range-scoped Usage are now
reachable Settings sections alongside committed Memory runtime preferences.
Model settings manages API-key providers and their models through `providerSettings/read|mutate` (see `PROVIDER_SETTINGS.md`). Subagents supports explicit opt-in local management through
the unchanged full Host, with preference handoff and truthful uncertain outcomes. Its rebuilt
inventory has inspected native captures, but full native actions/reference visual parity remain incomplete.
Skills/Commands/Hooks/Browser Use still require missing owner ports or further bounded work. Current provider/updateAccountConfig is an account overlay,
not an API-key provider settings write; connectivity testing does not persist credentials.
Skills/referenceCatalog and workspace/readPresentation are read-only presentation ports,
not Settings CRUD. Hooks trustGrant requires owner-supplied bundle/declaration digests
without a current GPUI Settings snapshot adapter. The optional mcp/list mcpServers field
must not be repurposed as a frontend configuration owner. Scoped live skill/plugin
catalogs, queue Edit/reorder, goal Pause/Resume, Personal Source controls, saved workflow
launch/definition/history, run settings and confirmed saved metadata/delete/move controls
are reachable; their presence does not establish complete lifecycle parity. Draft-safe
canonical plugin Example Prompt prefill is implemented. Non-path attachment upload,
workflow artifact detail remains audited. Plugin Details and scoped typed Configure/Clear/Reset
use existing CLI ports; remote-sync cancellation is excluded, not offered as local RPC cancellation. Task
index, auth and scheduling remain host-owned. Drafts survive navigation in memory, not
restart. Native visual/restart/cross-platform evidence, activation and distribution
remain incomplete. See COMPOSER_PARITY.md and WORKFLOWS.md for the later slice contracts.
