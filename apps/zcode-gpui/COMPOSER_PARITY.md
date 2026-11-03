# Composer and activity parity

Status: implementation in progress, 2026-10-06. This is the action contract for the
next bounded parity slices; `PARITY.md` retains the overall scope and exclusions.

## Background-work entry and child conversations

The reachable React composer has one combined background-work control, not one chat
icon per agent. It counts running Bash and workflow background works plus the joined
running-subagent projection. A composer wider than 480 px shows each nonzero kind with
its typed count inside one aggregate Ely control. The current pinned Ely Button accepts
only one leading icon, so mixed typed per-kind icon groups remain a presentation gap;
the labels distinguish the counts without a hand-rolled replacement. At 480 px or
narrower it shows Activity and the total. The boundary measures the composer container,
not the window. Resize changes presentation only. Zero active work hides the button. The GPUI equivalent opens the Review
dock and expands its activity sections; it never creates a second accepted-work list.
The panel offers per-agent Open and backend-supported per-work Stop controls.

Owners and event order:

```text
CLI snapshot/delta → ConversationState authoritative mirror
                  → derived composer count and activity rows
composer click → RootView dock navigation/expansion only
agent Open → captured workspace + parent + child association validation
           → AppState child navigation → existing child topic subscription
           → blur hidden parent input; read-only child view
Back → release only child subscription/mirror → focus unchanged parent composer
Stop → existing cancelBackgroundWork(workId) command → checked acknowledgement
     → authoritative stream updates activity; no optimistic removal
```

The existing parent subscription stays live. Draft text, attachment references and
temporary-file ownership remain with the shared parent composer. Child navigation does
not save, clear, or submit that composer. A control rendered for an older workspace or
parent must not open/reparent a child after navigation. Unknown/self/blank children are
rejected; membership must come from that parent's projection, background work, loaded
subagent row, or already established matching association. Opening another child
releases the previous child's subscription, never the parent. Runtime still rejects
child input; the view must also prevent hidden parent input or submission.

Running rows sort by start time ascending (missing first), then child ID for stable
ordering. Completed agents are not included in the running count. The ended directory
is a separate `session/subagents` query slice, not a fabricated running projection.
No physical-wire or desktop-continuous/web-remote-replayable semantics change.

### Ended subagent directory

The expanded Agents section queries the existing `session/subagents` method, with
only `sessionId`, optional `endedCursor`, and `endedLimit: 20`. Workspace selection
is the stdio connection boundary, not an extra payload field. `WorkspaceHandle`
owns connection-local, per-parent query pages; they never replace snapshot-owned
running rows, ended count, child IDs, or the sessions index. First-page loading,
empty success, error, Refresh and Load more are reachable. Errors retain cached rows.
Pages deduplicate by child ID in backend order. A snapshot subagent revision change
refreshes from page one and reads enough pages to preserve the previously loaded
depth before committing the replacement. Duplicate reads are suppressed per parent,
not globally, so changing parent can query immediately. Connection invalidation
clears the cache; pending correlation and process-generation gates reject old replies.
Refresh failure retries only explicitly or for a newer snapshot revision.

```text
parent snapshot revision → scoped page request → strict query decoding
                        → ended-page cache only → ended Open
                        → captured workspace/parent and cached membership validation
                        → existing read-only child subscription
```

An ended row can establish membership only from its originating cached query and
must still pass the existing conflicting-owner/stale-binding guards. No query result
mutates parent projection or navigates on arrival. Tests cover pagination, duplicate
suppression, refresh depth, cache retention on failure, wrong scope and reconnect,
and opening an ended child without modifying its parent's draft.

Acceptance scenarios:

- Zero work hides the control; agent-only, Bash-only, workflow-only and mixed counts
  match the backend projection without counting the same agent twice.
- Pointer activation opens Review even when another dock tab was selected, expands
  activity, leaves draft/focus intact, and sends no stop or input command.
- Agent Open and Back preserve text, attachments and the parent subscription; typing
  and Enter in a read-only child cannot alter/submit the hidden parent composer.
- Stale parent/workspace buttons and unknown child IDs do not change navigation or
  child ownership. Switching child releases the old child route.
- Per-work cancellation targets its own work/run ID, not foreground `stop`; duplicate
  pending clicks are disabled and rejection does not remove the work locally.
- Narrow layout keeps the aggregate control compact; labels/tooltips use the active
  locale and Ely semantic theme/typography. Native visual acceptance is separate from
  NoopTextSystem pointer/keyboard fixtures. Resize fixtures synchronize the pinned test
  platform's bounds into Window viewport before drawing and assert the effective size;
  wide–narrow–wide checks must not pass against stale initial display bounds.

## One-shot follow-up delivery

Ctrl+Enter on Windows/Linux and Cmd+Enter on macOS choose the opposite delivery for one
existing-session `sendText`: guide preference requests queue; queue preference requests
startNow. Modifier-click on Send does the same only when authoritative `control.canStop`
is true. Keyboard submission uses current `inputRouting`, not phase-derived idle state;
CLI still decides idle start and atomic foreground promotion. Shift+Enter remains newline
and IME composition never submits. Single-line nonchat inputs retain ordinary submission.

One typed submission trigger passes through the existing composer/submission owner and
sender. It does not persist a draft delivery preference, emit setFollowupMode, stop first,
or create another admission queue. Ordinary guide submission retains its current payload;
new-session firstInput, Edit/Rename and local slash actions keep their existing contracts.
The current ConversationState mirrors `canStop` and whole-key inputRouting replacements.
Reject routing retains text/attachments and shows a reason. Choice routing opens the
Clear/Keep confirmation described below; no disposition is guessed.
Missing routing cannot authorize modified Enter or a modifier-click that would reverse
delivery. A modifier-click with canStop false remains an ordinary click, including when
routing is missing. A changed followup preference is read at click time, not captured as
an old rendered preference. Refused routing leaves the composer replacement generation
and explicit temporary-file ownership untouched; failed enqueue restores that ownership.
Rejected admission merges attachment references into the originating draft without
reclaiming already-retired local files or automatically replaying the delivery override.
The banner names the ACK status and `reasonCode` (and the CLI message, when present), and
the same line goes to the in-memory log (Export logs), because the CLI does not log
guard/stale refusals itself (e.g. `guard.runtimeInitializing`, `proto.sessionNotFound`).

```text
Enter / modified Enter / Send click → typed one-shot trigger → current projection gate
    → existing submission ownership transfer → sendText.requestedDelivery
    → CLI foreground promotion / CommandInbox admission → authoritative ACK/stream
failed enqueue / rejected / stale → originating draft recovery, no automatic retry
```

Acceptance covers ordinary and opposite guide/queue payloads, platform modifiers, pointer
canStop gating, reject/choice/missing routing, snapshot/delta replacement, unchanged session
preference, new-session/slash/intent compatibility, failed sends and rejection with newer
drafts/attachments. TestApp pointer/keyboard evidence is not native rendering acceptance;
real CLI smoke verifies explicit idle startNow admission/result identity and actual model
frames separately. Queue/guide mapping is covered by frontend payload tests, not a claimed
real-CLI busy-promotion/steering end-to-end scenario.

## Held-queue Clear/Keep confirmation

For a writable parent plain sendText or /goal <new objective> with current inputRouting
choice, the existing
submission owner opens an inline Ely Clear/Keep/Cancel confirmation before transferring
composer ownership. AppState owns one transient immutable receipt, not a second draft
or accepted queue. It captures workspace identity, session, connection/navigation and
composer replacement generations, current text/attachment/config fingerprints, original
submission trigger and the projected queue-item ID set. No new file persistence or
runtime owner is introduced. New goal objectives use sendGoalCommand with the same
reviewed-ID/disposition fields, never one-shot requestedDelivery. /goal resume and
/compact retain their control-command paths and do not open this confirmation.

Clear and Keep send the existing sendText/sendGoalCommand fields heldQueueDisposition
and expectedHeldQueueItemIds. Plain sendText carries the original one-shot
requestedDelivery if any; a goal command never receives that field.
Every click validates the current writable owner, unchanged composer/config and ready
connection. A changed queue requires another explicit confirmation with current IDs;
a changed draft, navigation or connection cancels the stale receipt without sending or
overwriting the draft. Cancel emits nothing. Duplicate or retired callbacks emit nothing.
CLI owns queue disposition and admission; neither ACK nor confirmation removes projected
queue rows optimistically. Existing enqueue and rejected-input recovery owns attachment
lifetimes and pending input, with no automatic replay.

A failed ACK with guard.heldQueueConfirmationStale restores/merges the originating input
through existing recovery and reopens confirmation only for that currently bound writable
parent with choice routing. It captures the newest projected queue IDs and requires a
new click. Unknown outcomes do not reopen a resend action. No timeout is used to invent
state or await an assumed stream update. Desktop live and replayable stream semantics
remain unchanged.

```text
plain Send + choice → AppState receipt → unchanged composer + Clear/Keep/Cancel
choice click → owner/draft/current queue check → existing submission transfer
             → sendText disposition + reviewed IDs → CLI queue guard/admission
stale guard ACK → origin draft recovery → current receipt → explicit new choice
ACK/stream → only authoritative projection changes accepted queue rows
```

Acceptance: pointer Clear/Keep/Cancel, untouched draft/attachments before choice,
exact disposition/ID payload, duplicate suppression, queue/draft/config/connection and
workspace invalidation, stale ACK recovery without replay, accepted ACK without
optimistic queue mutation and failed enqueue ownership preservation. The isolated CLI
fixture explicitly holds a loopback model response, pauses queue auto-drain, admits a
queued input, and releases the response to reach authoritative choice routing. It then
checks missing/stale disposition refusal, Keep retention and Clear removal through fresh
strict snapshots. Only harness-created queue items are cleared. Observation deadlines
bound the smoke; they are not production synchronization or fabricated routing state.
Native rendering and GPUI-executable-to-real-CLI held-queue execution remain separate gaps.

## CLI-owned slash command discovery

The `/` popup reads a connection-local catalog owned by `WorkspaceHandle`, keyed by
optional session ID. Drafts use the existing `workspace/readPresentation` with path for
file access and key for identity. Existing parents use `session/read` with sessionId and
messageLimit: 1, never a workspace fallback. Only the returned slashCommands are retained;
legacy messages, runtime, settings and deliveryKind are not applied to the V4 mirror. This
read does not subscribe, create a session, change mode or alter delivery semantics.

Each scoped query has Idle/Loading/Ready/Failed state. Loading/error hides old suggestions,
empty success stays empty, and Refresh is explicit after settlement. There is no hard-coded
built-in merge: owner filtering, reserved-name precedence and disabled workflow/custom
commands cannot be undone by a frontend fallback. The workspace-config topic remains a
separate projection; its slash list uses whole replacement, including empty/missing keys,
and is not a second composer catalog owner. Strict command decoding checks name,
description, optional inputHint and builtin/custom source before committing the list.
Workspace replies must echo their requested path/key; session replies must identify the
requested session and workspace. Missing session slashCommands is unavailable, not a
license to use workspace commands. Unknown protocol command fields fail closed.

`/init` is suggested when present in that owner's catalog. Selection inserts `/init ` only;
normal sendText/firstInput passes the raw command and optional notes to the CLI's existing
built-in prompt resolver. GPUI does not copy an init prompt, write AGENTS.md itself, or
create another command resolver. Prompt-backed custom commands keep that same admission
path. `/plan` uses the local submission contract below; listing other commands does not
establish unrelated native/host action parity.

```text
current slash query → AppState scoped read → existing stdio pending registry
    draft: workspace/readPresentation → requested workspace echo
    parent: session/read → requested session/workspace → slashCommands only
    → strict decoding → originating WorkspaceHandle scoped query → `/` suggestions
selection → captured owner/process/navigation/query/text receipt + current membership
          → same composer insertion → user Send → existing CLI prompt/admission owner
refresh/reconnect/owner or text change → old selection cannot overwrite newer input
```

Every slash suggestion and Refresh callback captures workspace, optional parent, process
and navigation generations, query revision and original composer text. Changed scope,
query, IME composition or draft text invalidates it. Selection never submits and preserves
attachments/deletion ownership. Query errors are generic before storage, not arbitrary
server text. Connection invalidation clears both pending reads and scoped caches; late
replies cannot recreate old catalogs. No automatic retry or timer-derived capability is
introduced; desktop-continuous and web-remote-replayable still share CLI admission.

Acceptance: `/init` and custom discovery for draft/parent, exact scope payloads, no session
fallback, loading/empty/error and Refresh, strict malformed/wrong-owner refusal, stale
selection/refresh, reconnect, untouched attachments and raw `/init` submission/recovery.
TestApp pointer/keyboard coverage is separate from native visual acceptance. Isolated CLI
smoke reads both real schemas with a disposable custom command and observes `/init` prompt
expansion against the loopback model without granting model tools to modify real files.

## Local `/plan` draft and ordinary submission

`/plan` without arguments enables plan in the existing per-draft submission override,
clears the consumed command and sends nothing. It does not switch live session mode,
create a session, or need a connected backend. Its scope survives ordinary navigation
through that same map, but is not persisted across restart and is consumed after a
successful submission, like other overrides. Selectors overlay it on current draft/live
model defaults rather than replacing missing model fields; draft model edits stay visible,
and mode edits use the existing override when present instead of a competing UI value.
Thinking-level selection also edits an existing draft override before any live-session
command gate; Default removes its explicit reasoningLevel while preserving other model
options. A plain draft without an override keeps its existing no-op thinking behavior.
Rejection returns the frozen override to
its originating draft only if no newer override exists. This bounded local behavior is
not a claim of React's durable composer-preference parity across successful turns.

`/plan <task>` strips only the outer command/argument whitespace, preserving internal
spaces/newlines. It sets planEnabled:true in the current override, preserves a valid
non-plan base mode (build/edit/yolo), and normalizes plan to build. Existing modelSelection
and other restored overrides stay intact. The task goes directly through ordinary
sendText, or createSession.firstInput plus config for a new draft. It is not recursively
classified: `/plan /compact` submits the literal task `/compact`, not a compact command.
Both firstInput and config carry the same frozen plan/mode intent. No separate mode CAS,
stop-first command, second sender or optimistic live configuration is introduced.

Attachments and structured reference chips block `/plan` before ownership transfer,
config changes or held confirmation; text, chips, temp ownership and config remain intact
with localized feedback. Read-only child/Edit/Rename/IME contexts cannot use this local
shortcut to mutate hidden parent preferences. Other mode-like slash names (/build, /edit,
/yolo, /auto) remain ordinary CLI/custom text, matching the current app parser.

```text
/plan → existing AppState draft override → clear command only → later ordinary submission
/plan task → payload gate → frozen override + ordinary-input routing
    → held choice receipt if needed (no transfer before Clear/Keep)
    → same sender → stripped task + planEnabled/mode → CLI admission/stream
failed enqueue/rejected → original /plan text + original scope override, no replay
```

One-shot modifiers and held-queue choice follow ordinary input semantics for a nonempty
plan task. A stale held guard restores original `/plan` text and requires a new reviewed-ID
choice. Bare `/plan` never opens queue disposition. Cancel retains unsent text/config.
Acceptance: bare draft/parent no RPC, attachment/chip/IME/read-only refusal, normalized
firstInput/config and sendText, no recursive slash handling, modified delivery, held
Clear/Keep/stale behavior, failed enqueue/rejection and newer override preservation.
Pointer Send and keyboard tests are separate from real CLI plan execution and native
visual acceptance; only isolated scratch fixtures may exercise actual admission.

## Existing-chat clipboard-image upload

A nonempty image paste in a writable existing parent uses the existing attachment
begin/chunk/commit/abort wire methods. `AppState` coordinates one connection-local
upload registry on the originating `WorkspaceHandle`; each entry owns its source bytes,
preparation/progress/error state and an immutable workspace/session/process/navigation/
composer-replacement receipt. Hashing and base64 preparation run on the background
executor. The registry is bounded by the protocol's per-file, count and staged-byte limits.
No accepted-input queue, session, trusted Host lease or configuration owner is created.

```text
clipboard bytes → composer event → current writable parent receipt → upload owner
    → background checksum/chunks → begin → sequential checked chunks → commit
    → same current composer attachment ref → explicit user Send → CLI admission
Retry → fresh uploadId for retained bytes; Cancel → retire receipt + best-effort abort
navigation/replacement/reconnect → retire old ownership; no late attachment adoption
```

Send and `/plan` preparation are blocked while a current upload is preparing, staging
or failed; they do not transfer or clear the draft. The inline Ely status offers Cancel
and, after a failure, explicit Retry. Errors are generic before storage. No automatic
retry or ACK-derived input admission occurs. Begin/chunk results must identify the
requested upload and valid progress; commit must be a strict nonblank artifact ref.
Only the ready committed ref enters the composer, without local temp deletion ownership.
Clipboard bytes retain their declared GPUI image format and matching extension/MIME;
neither uploads nor new-chat temporary paths may relabel JPEG/BMP content as PNG.

Cancellation attempts abort on the original still-current connection and retires pending
correlation. Abort cannot roll back an already committed artifact; it only prevents late
adoption. Connection invalidation retires upload state and pending calls; CLI disconnect/
TTL cleanup remains authoritative. New-chat paste keeps the existing unique owned-temp
path because uploads require an existing session; picker/drop paths remain zero-copy.
Read-only child and Edit/Rename contexts cannot upload into a hidden parent. No remote
upload owner or draft-session creation is added by this bounded slice.

Acceptance covers clipboard keyboard reachability, bounded metadata/chunks/checksum,
strict wrong-ID/progress/ref refusal, Cancel/Retry, navigation/replacement/reconnect,
failed enqueue, generic errors and attachment-preserving blocked Send. Isolated CLI
smoke uses a vision-capable loopback fixture to verify a scratch transaction and later
image-ref resolution into a model request; native image display,
full executable E2E and new-chat non-path upload remain separate evidence gaps.

## Remaining bounded slices

Queue Edit captures only queued sendText/sendGoalCommand items and requests
`deleteQueueItem` with the mirrored revision. A dedicated pending request owns the
restore payload until accepted/duplicate ACK; noop/rejected/stale/failed/unknown never
restore and are not retried. The authoritative queue is not optimistically removed.
Empty, writable composer and queueEdit availability are required before dispatch.
After ACK, workspace/session/navigation generation and empty composer are rechecked;
a changed binding retains the recovered item as that originating session's draft,
never overwrites the visible composer. Existing newer drafts are merged, not erased.
Restored session-owned attachments must not be claimed as local temporary files.
Only the local paste producer transfers deletion ownership to the composer; picking,
queue restoration, draft recovery and rejected-submission recovery add references
without inferring ownership from filenames. Removal deletes only an explicitly owned
unsent paste, on the background executor. Each paste materialization has a unique
path, so removing a later paste cannot delete a path already retained by an accepted
submission. Session-owned references remain usable through restore, removal and
resubmission; app-retired local files stay under the existing exit cleanup owner.
Asynchronous paste completion captures the composer replacement generation. A send,
navigation or intent replacement invalidates the completion and cleans its unadopted
file rather than attaching it to a different draft. Saving a draft transfers its
attachment references into the existing per-draft recovery map and retains its local
paste files under the app exit owner; restoring replaces, rather than merges, another
draft's visible references.

`AppState` owns one per-draft submission-override map for restored mode/planEnabled/
modelSelection. It is separate from the live session configuration. Selectors render
and edit that draft override, and the next input carries it through the existing
command payload. Successful enqueue transfers the override into Pending::Submission;
explicit rejection returns it to its origin only if no newer override exists. No
backend config command is emitted merely to restore an item. Compact is not editable.

````text
Queue Edit → pending delete + captured restoration → accepted/duplicate ACK
           → matching empty composer or originating saved draft
           → per-draft submission override → next input admission
``` Goal state/actions
must be projected from snapshot availability and use mirrored revision CAS. Live
`skills/referenceCatalog` and `plugins/referenceCatalog` replace hard-coded mentions;
existing-session authority must never fall back to draft-workspace authority on error.
Saved workflow start/amend and plugin lifecycle controls use current protocol owners.
Workflow Resume requires `resumable == true`, a nonsuperseded stopped run and the
captured current writable workspace/parent. It never infers permission from stopped
status. One pending resume per run disables repeated activation, with no optimistic
status and no CAS revision. Runtime capability/rejection errors remain visible.
Local-path attachments remain valid direct references; chunk upload is only needed for
non-path content, not a mandatory replacement for the native local picker.

### Goal projection and controls

`ConversationState` mirrors snapshot/delta goal and pauseGoal/resumeGoal availability.
Review shows objective, authoritative status and iteration. Pause/Resume are enabled
only when that action is allowed, a revision is known, the view is a writable parent
and no identical command is pending. Click callbacks capture workspace/session and
revalidate before using the existing CAS command. `/goal resume` is a resume action,
not a replacement objective; failures retain the submitted composer text. No optimistic
goal status is written. Missing availability fails closed. Snapshot replacement and
explicit null remove old goal facts; patches replace complete goal/availability keys.
Tests cover state replacement, unavailable/no-revision/read-only guards, pending
duplicates, correct CAS payload and rejected acknowledgements.

### Live reference catalogs

`WorkspaceHandle` owns a connection-local catalog cache keyed by optional session ID;
each skill/plugin request has Idle, Loading, Ready or Failed state. Requests use that
workspace's file path plus identity key and the existing optional sessionId. Existing
sessions require response authority `session`; drafts require `workspace`. An error or
wrong authority never enables hard-coded suggestions or retries under another owner.
Connection reset clears these caches. Background results may populate only their
captured workspace/session cache; the composer reads only its current scope. Explicit
Refresh starts a new read after settlement. No IO occurs synchronously in catalog render.
`$` searches skills; `@` searches plugins plus existing file/session mentions. Canonical
insertions are `[$name](path)` and `[@name](plugin://pluginId)`, represented as chips.
Skill and workspace paths never carry the Windows `\\?\` prefix (stripped where the
workspace path is created and again when a link is built), so an absolute skill path
serializes as `[$name](C:\\dir\\SKILL.md)` exactly like Electron.

**Chip display (2026-10-29).** The composer string and the sent text stay the raw
markdown; only the rendering changes, mirroring Electron's `parseMentionMarkdown` /
`ConversationUserInputContent` (`shared/mention_parse.rs`, `composer/chip_display.rs`). Links and bare tokens
(`$skill`, `#sess_…`) render as a pill: an icon and the label (session `#`, skill `⚡`
with the slug in Title Case, file `📄` with the file name, folder `📁`, plugin `🔌`). The
composer maps caret, selection, hit testing and IME positions between raw and displayed
text through the chip table; chips stay atomic. The sent bubble shows the same pills;
Copy copies the raw markdown.

**@ file index (2026-10-29).** Files come from a per-workspace index built on a background
thread (`composer/file_index.rs`, `ignore` crate): `.gitignore`, `.ignore` and
`.zcodeignore` are honored, with no depth or count limit, and `.git`, `node_modules`,
`target`, `Library`, `Temp`, `Logs`, `UserSettings`, `obj` plus `*.meta` files are skipped.
The previous per-keystroke walk on the UI thread stopped at depth 4 / ~800 entries, so deep
files in large (Unity) projects never appeared. The index is built when `@` is first typed
in a workspace ("Indexing…" row meanwhile), rebuilt by the next search ≥500 ms after a file-watcher event (`notify`), and
rebuilt once when a query finds nothing.

Ranking is fzf-style (`composer/fuzzy.rs`, 2026-10-29), a deliberate improvement over
Desktop's prefix/substring/subsequence scorer (`packages/shared/src/workspaceFileSearch.ts`);
the inserted markdown is identical. The query's characters must appear in order,
case-insensitively. Each entry is scored by the best alignment, found with a dynamic
program, both on its file name (+8) and on its whole relative path. Matches at word starts
score highest: the start, after a space, after `/`, after `_` `-` `.`, and on camelCase or
digit humps. Consecutive runs keep the bonus of the run's start, and gaps cost
`-3` then `-1` per character. Ties go to the shorter path, then index order. There is no
typo tolerance. Folders insert as `[name](./rel/)`.

The popup lists up to 6 files and folders, then sessions (8 rows in total). Results are
memoized per query. Indexes of up to 20k entries rank synchronously, which takes a few ms.
Larger ones rank on a background thread: the previous query's hits stay visible, or
"Searching..." when there are none, and the popup re-checks every 50 ms. The walk, rebuilds
and large rankings never block the UI.
Disabled or conflicting plugins are never insertable. Loading, errors and an empty
catalog are visible in the popup, with a retry/refresh control. Tests cover strict
response decoding, wrong authority, session/draft scope, stale navigation, conflict
filtering and canonical insertion. Catalogs are read-only and not Settings management.

### Personal sources and manual refresh

Plugin Store Personal Sources exposes a separate single-line source draft. Add and
Manual Refresh use existing marketplace/add and marketplace/update, not overview-only
refresh. The handler captures and validates workspace identity. One mutating plugin
operation per workspace uses the existing pending registry; it never makes optimistic
catalog changes. Duplicate dispatch is suppressed, pending controls are disabled, and
results refresh only the originating overview. Error-severity diagnostics and source
refreshFailure are sanitized and shown as failures even inside a successful RPC.
An add source draft is frontend-local, never an application setting; no URL/token is
logged. It survives connection invalidation and remains available after Add success or
failure, so a backend reply never erases newer edits. Local path picker results capture
workspace, navigation generation, input entity and original text. Cancellation leaves
text unchanged; a changed binding or edited input discards the result. Picker loading
and sanitized errors have the same workspace-local draft owner, not the query cache.

Install/update/toggle/uninstall/restore/remove and Add/Manual Refresh share one mutation
sender and pending registry. Every callback captures its rendering workspace; the sender
rechecks that owner, connection readiness and capability before dispatch. Workspace path
is used for file access and workspace key for identity. Failures leave committed rows
intact. Transport errors, missing/malformed results, unsupported capability, error
severity diagnostics and persisted source refresh failures cannot be reported as success.
The overview query is separate from mutation feedback: a later overview read cannot
clear an operation failure. Settings Plugins stays in Manage Installed and does not
expose marketplace tabs. Cancel/configure/describe are subsequent supported protocol
slices, not inferred operation progress.

```text
source input/picker → workspace-local draft (survives reconnect)
rendered mutation → captured workspace → single sender + pending registry
                  → backend-owned mutation → validated result / sanitized feedback
                  → originating overview refresh → derived catalog rows
connection reset → discard pending/query feedback, retain unsent source draft
````

Acceptance includes pointer Add/Manual Refresh, pending duplicate suppression, stale
workspace callbacks, identity distinct from file path, failed enqueue, Add draft retention,
error diagnostics inside a transport success, source refreshFailure projection, picker
cancel/stale/edit guards, and Settings-only management navigation.

Personal Segment groups each source card with that marketplace's available plugin
cards; it must not hide Personal Source install/Example Prompt controls behind a
source-summary-only view. The ordinary available-card mutation/prompt gates apply.

### Plugin Example Prompt safety

Example Prompt never replaces the current composer and never sends a message. The
callback captures workspace/plugin/prompt. An uninstalled card dispatches its existing
scoped Install only; the user activates the prompt again after installation. An installed
card queries the existing workspace-authority `plugins/referenceCatalog` before creating
a project-scoped draft. Only a currently enabled, conflict-free canonical plugin reference
is inserted as an atomic chip, followed by the trimmed prompt. This does not implement
the separate official recommended-prompt resolver or automatic enable/install flow.
One prompt query per workspace is pending; readiness, writable parent, current catalog
membership and pending mutation gates are checked at click. The reply rechecks navigation
generation, originating text and attachment references and cannot steal a newer edit.
An existing nonempty project draft (text, attachments or submission override) blocks the
action with visible feedback, rather than silently replacing the only workspace draft.
The prior parent composer/draft remains recoverable through normal navigation. Errors,
missing/disabled/conflicting references and stale replies do not create or prefill a chat.
Connection reset drops pending queries without replay. Card versus full detail-page
presentation remains a parity gap, not permission to overwrite draft ownership.

```text
Example Prompt → uninstalled: scoped Install only
               → installed: captured owner + workspace reference query
               → matching generation/input + enabled conflict-free reference
               → empty project draft only → normal navigation saves parent draft
               → canonical chip + prefill → user submission is the admission boundary
```

No provider/capability Settings file adapter or second scheduler is introduced here.
The current CLI lacks provider/skill/hook/subagent-management Settings ports; those
remain explicit owner-integration blockers under the zero-backend-fork guardrail.
