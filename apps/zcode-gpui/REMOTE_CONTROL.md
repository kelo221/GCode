# Phone remote control: native Android client over the local network

Status: **in progress**. The LAN slice works end to end on a Pixel 7a against a Windows
desktop (2026-10-15, see "LAN slice" › Verified). The cargo feature split, the remaining phone
UI work and hardening are still open.
This file is the spec for the feature. Update it before changing behaviour.

## Product rules

- The phone runs **the same GPUI app** (an Android build of `zcode-gpui`) as a remote
  screen for a desktop `zcode-gpui` instance on the same network. It never runs an agent,
  CLI or Host itself (AGENTS.md: the phone reuses the existing host and runtime).
- **Local network only** for the first version. No external relay, no Z.AI account, and
  no official phone web page. Off-network use means the user's own tunnel (e.g. Tailscale).
- Off by default on the desktop. Pairing uses a QR code shown by the desktop. One phone
  is paired at a time; the desktop can revoke it.
- **Phone scope:**
  - list and open sessions, with a live transcript;
  - send, steer and stop;
  - answer permission, question and plan prompts;
  - attach images.

  Files, git review, terminal, plugins, MCP and subagent management stay desktop-only.
- **Browser Use from the phone (2026-10-15):** a turn started on the phone runs on the
  desktop's agent, so its browser tool drives the desktop's browser through the desktop's
  extension bridge (`BROWSER_BRIDGE.md`). The phone has no bridge of its own, and the phone
  build hides Settings › Browser Use.

## Architecture and owners

```text
Android phone: zcode-gpui (remote-client build)         Desktop: zcode-gpui (local-backend build)
 UI, frame assembler, commands (unchanged)               ┌─────────────────────────────────┐
 RemoteConnection ──TLS, pinned cert fingerprint────▶   │ LAN gateway (off by default)     │
   one socket per open workspace,                        │  pairing / device-token auth     │
   NDJSON V4 lines, same as the CLI stdio wire           │  per-phone remote scope          │
                                                         └──────────────┬──────────────────┘
                                                                        │ existing stdio
                                                         CLI app-server (one per workspace)
```

| State | Owner |
|---|---|
| Accepted input order | CLI `CommandInbox`. Desktop and phone are peers; neither UI orders the other's input. |
| CLI process, initialize result, reverse RPC answers | Desktop `zcode-gpui` (as today). The phone never receives reverse RPC. |
| Phone subscriptions (`subscriptionId` ownership, held early frames) | Desktop remote scope, per phone connection. Dropped on disconnect. |
| `connectionId`, `clientMode` | Set by the desktop scope (`gpui-lan-<device>-<uuid>`, `web-remote-replayable`). Never taken from the phone. |
| Pairing secret | Phone: device token in app-private storage. Desktop: SHA-256 of it only. |

Event order:

```text
phone hello/initialize ─▶ scope answers from the desktop's cached initialize result (CLI not re-initialised)
phone subscribe ─▶ scope: set identity and mode, remap request id, hold that topic's early frames ─▶ CLI
CLI frame(subId) ─▶ held (ack pending) | phone socket (phone-owned subId) | desktop assembler (own)
CLI ack{subscriptionId} ─▶ scope records ownership and releases held frames in order
phone command / resolveInteraction ─▶ CLI CommandInbox
socket drop / revoke / workspace closed ─▶ unsubscribe everything the phone owned, drop held frames
phone reconnects ─▶ subscribes again ─▶ snapshot (resume-from-seq is a later optimisation)
```

## Interfaces

- **Transport.** TCP + TLS (rustls, self-signed desktop certificate). The phone accepts
  only the certificate fingerprint from the QR code. Lines are capped by `MAX_RAW_LINE_BYTES`.
- **Socket handshake.** The first line is `{"type":"pair","pairingToken","deviceName"}` or
  `{"type":"auth","deviceToken"}`. The desktop replies with the workspace list. Then
  `{"type":"open","workspaceKey"}` switches the socket to raw V4 lines for that workspace.
- **QR / link.** `zcode-gpui://pair?host=…&port=…&fp=<sha256>&t=<one-time token, 5 min>`.
  Scanning with the phone's camera app opens the link in the app.
- **Method allowlist (desktop scope):**
  - **Allowed:**
    - `v4/conversation/subscribe`, `unsubscribe`, `resync`;
    - `v4/command` for chat commands: send, steer, stop, `resolveInteraction`, mode/model/thought, session list/open/rename;
    - flow control;
    - attachment upload and read.
  - **Everything else** gets an immediate JSON-RPC error. This includes the model-catalog
    probe that creates and deletes a session; the desktop serves the catalog from its cache.
- **Client seam.** `backend/conn.rs::Connection` (event receiver, `inbound` sender, `kill`) is
  produced either by `spawn_connection` (local) or by `remote_client::connect` (TLS). Code
  above it is unchanged. `clientMode` comes from the connection type instead of being
  hardcoded in `backend/commands.rs` and `flow_cmds.rs`.

## Phases

0. **Spikes:**
   - (1) Android build on our pinned gpui: **done**.
   - (2) Soft keyboard: **done**.
   - (3) Wire test: a second `connectionId` with `web-remote-replayable` on one CLI, against a fake CLI with two subscribers.
1. **Library split.**
   - **Done:** the crate is a library (`src/lib.rs`) with a thin desktop `main.rs`, plus
     `load_startup(workspace)` and `open_window(cx, startup)` entry points shared by both builds.
   - **Still to do:** gate desktop-only modules behind a `local-backend` feature (process
     launch, services_rpc, terminal, git review, files, folder picker, OS integrations), add a
     `remote-client` feature, and check it with
     `cargo check --no-default-features --features remote-client`.
2. **Desktop remote scope** (`backend/remote_scope/`, ≤400 lines per file):
   - handshake, id remap, identity/mode injection, routing by `subscriptionId` before reassembly (1024 frames / 32 MiB held), allowlist, cleanup;
   - a phone request for a workspace that isn't loaded starts the desktop's own backend (`ensure_spawned`).
   - Tests: port `zcodeAgentConnectionScope.test.ts`, plus both stream semantics on one session.
3. **Desktop listener and pairing:**
   - the listener is bound to a chosen LAN interface;
   - pairing tokens and device tokens as specified above, with repeated auth failures rate-limited;
   - a Settings › Phone page (toggle, QR, revoke) and a header status chip;
   - tokens and links never logged.
4. **Android app** (`apps/zcode-gpui-android`):
   - **Done:** cdylib, platform layer, Kotlin shell, Gradle and the deploy script.
   - **Still to do:** the `zcode-gpui://pair` intent filter, the pinned-fingerprint TLS client and reconnect.
5. **Phone UI:**
   - phone layout (see "Phone layout" below);
   - hide desktop-only panes;
   - make hover-only controls visible on touch;
   - turn the sidebar into a drawer;
   - lift the composer above the keyboard;
   - show "reconnecting" / "desktop offline" banners.
6. **Hardening:**
   - desktop and phone driving one session at once;
   - screen-off and Wi-Fi loss mid-stream;
   - a CLI restart;
   - revocation, a wrong certificate fingerprint, oversized lines.

## LAN slice (2026-10-15)

The first connected version. Where it differs from the plan above, this section wins.

**Protocol facts it relies on** (CLI `apps/zcode-cli/packages/bootstrap/src/zcode-protocol/`):

- Over stdio the CLI writes a subscribe response (`{ack:{subscriptionId, mode, logEpoch}}`)
  **before** that subscription's first frame (`transport.ts` response, then the post-response
  outbox). The desktop scope sits on the raw stdio lines, so it learns a phone subscription's
  id from the ack before any of its frames, and **does not need to hold early frames**. The TS
  scope holds them only because it sits above an RPC facade.
- CLI→client requests use string ids (`server-N`) and always go to the desktop.
- No `initialize` is needed on stdio. Readiness is the `startup/storageState` `phase:"ready"`
  notification.
- `clientMode:"web-remote-replayable"` selects the CLI's replayable delivery profile (slower
  flush, no desktop-only rows), which is what a phone wants.
- `v4/connection/flow` with `state:"closed"` drops a connection's flow state and uploads.

**Desktop owners.** `AppState` owns the gateway: the pairing store, the open phone
connections and one `RemoteScope` per open phone socket. Listener threads only do TLS,
authentication and line IO, and send events to `AppState`. Nothing else holds phone state.

```text
listener thread                AppState (gpui thread)                    CLI stdin/stdout
TLS accept + auth ──Authed────▶ reply {"type":"workspaces"}
phone line ─────────Line──────▶ {"type":"open"} ─▶ ensure_spawned(ws) ─▶ (spawn if idle)
                               storage ready ─▶ phone gets startup/storageState ready
phone line ─────────Line──────▶ scope.from_phone: allowlist, id remap,
                                connectionId/clientMode injected ──────────▶ stdin
                               stdout line ─▶ scope.from_cli (before the desktop parses it):
                                id ≥ REMOTE_ID_BASE ─▶ phone (id restored) or dropped
                                frame of a phone-owned subscriptionId ─▶ phone
                                everything else ─▶ the desktop, as before
socket closed ──────Closed────▶ scope.close: flow closed + unsubscribe all ─▶ stdin
                               CLI exit / unload ─▶ phone sockets for that workspace closed
```

- **Ids.** Remote request ids come from one counter starting at 2^48 (exact in JS numbers),
  so they never collide with the desktop's ids, which start at 1. A response with such an id
  that no scope owns any more is dropped.
- **Allowlist.** Methods: `v4/conversation/{subscribe,unsubscribe,resync,rowsRange,
  attachmentRead,attachmentStat}`, `v4/connection/flow`, `v4/attachment/{begin,chunk,commit,
  abort,read,previewSource}`, and `v4/command`. Subscribe topics: `conversation/*`, plus
  `sessions-index/<key>` and `workspace-config/<key>` of the opened workspace only.
  `v4/command` types: `createSession, sendText, sendGoalCommand, stop, compact, retryTurn,
  editUserQuery, setAssistantFeedback, sendQueuedNow, editQueueItem, reorderQueueItem,
  deleteQueueItem, setAutoDrain, resolveInteraction, snoozeInteractionAutoResolution,
  switchModelConfig, switchCollaborationMode, setFollowupMode, pauseGoal, resumeGoal,
  cancelBackgroundWork, renameSession`. Everything else, including `deleteSession`,
  file rewind, workspace hook trust and workflows, gets JSON-RPC error `-32601` with
  `data.phoneScope: true`. The phone settles such a request quietly (its panel shows
  "unavailable"); it is never an error banner or a workspace error, because the phone UI
  still asks for desktop-only panels such as the subagent directory on its own.
- **Model catalog.** The phone's `session/create` probe is answered by the desktop from the
  `settings` of its own probe, without a session id, so the phone never creates or deletes
  a throwaway session. A phone that opens a workspace whose agent has only just started asks
  before the desktop has that catalog; the gateway holds the probe and answers it when the
  desktop's own probe returns (an empty answer left the phone without a model picker).
- **Identity.** Every `connectionId` in phone params is replaced by the scope's
  `gpui-lan-<uuid>`, and subscribe's `clientMode` by `web-remote-replayable`. The phone
  build keeps sending its usual values; they are never trusted.
- **Backpressure.** If more than 32 MiB is queued for one phone socket, the desktop closes it
  (the phone reconnects and re-subscribes to a fresh snapshot).
- **Phone activity** counts as workspace activity, so idle unload leaves it alone.

**Transport and pairing.**

- TCP port 47821 on the desktop's LAN address (the interface of its default route), TLS 1.3
  via rustls with the `ring` provider. The desktop's self-signed certificate (rcgen) is
  created once and stored in the data dir. The phone accepts only the SHA-256 fingerprint
  of that certificate from the pairing link.
- Lines are newline-delimited and capped at `MAX_RAW_LINE_BYTES` in both directions. An
  overlong line closes the socket.
- Handshake: the first line must arrive within 10 s and is `pair` or `auth`. The reply is
  `{"type":"paired","deviceToken"}` (pairing only), then `{"type":"workspaces","workspaces":
  [{key,display,purpose}]}`. The next line is either `{"type":"open","workspaceKey"}`,
  answered with `{"type":"opened"}` after which both sides switch to raw V4 lines, or the
  phone closes (list-only socket). Errors are
  `{"type":"error","code"}` followed by close.
- Pairing tokens are 32 random bytes, single-use, valid for 5 minutes. A new pairing replaces
  the previously paired phone. The desktop stores only the SHA-256 of the device token, with
  the device name and pairing time, in `phone-remote.json` in the data dir, plus whether the
  listener is enabled (default off).
- More than 5 failed `pair`/`auth` attempts from one IP within a minute are refused until the
  minute passes.
- Tokens and links are never logged.

**Desktop Settings › Phone** (last in the Data and statistics group): an enable switch, the listen
address, the pairing link with a QR code and a copy button (a fresh link per click, valid 5
minutes), the paired phone's name with a Revoke button, and the number of connected phone
sockets. Revoke drops the stored device and closes its sockets.

**Phone (`remote-client`).** Selected at runtime on Android, not with a cargo feature yet (the
`local-backend`/`remote-client` feature split stays open in phase 1).

- `Startup::remote_client()` makes `AppState` start with no local workspaces and no local
  agent. It reads the pairing from `$HOME/remote-pairing.json`
  (`{host,port,fingerprint,deviceToken}`) and fetches the desktop's workspace list over a
  list-only socket. Each desktop workspace becomes a `WorkspaceHandle` with the desktop's key
  and display name and a remote target instead of launch candidates.
- `WorkspaceHandle::try_spawn` with a remote target calls `remote::client::connect`, which
  returns a `Connection` at once. TLS, `auth` and `open` run on its IO thread; failures arrive
  as a log line plus `Exited`. The rest of the app is unchanged.
- Reconnect: a remote workspace never runs out of candidates. After a drop it retries with
  backoff (1 s doubling to 15 s), showing "desktop offline" before the first successful start
  and "reconnecting to desktop" after. A re-subscribe brings a fresh snapshot.
- Pairing link: `zcode-gpui://pair?host=…&port=…&fp=…&t=…`. Android delivers it through an
  intent filter on `MainActivity` (camera QR scan or `adb shell am start -d`) and a JNI call
  into Rust. Settings › Phone on the phone also accepts a pasted link. Pairing sends
  `pair`, stores the returned device token, then loads the workspace list.
- When the phone isn't paired, the sidebar shows no workspaces and an error banner explains
  how to pair. Settings › Phone on the phone shows the paired desktop, "Pair from clipboard"
  and "Forget desktop".
- The dock (git review, files, workflows, plugins, MCP) and the terminal act on the device's
  own files, so the phone build hides their header toggles and never renders them, including
  the dock's automatic opening for a turn with a plan (`RootView::desktop_tools`).

**Code.** `src/remote/`: `scope` (allowlist, ids, routing), `gateway` and `gateway_controls`
(desktop owner in `AppState`), `listener`, `pairing`, `tls`, `handshake`, `line_socket`,
`link`, `client` and `phone_state` (phone side). Hooks: `backend/events.rs` routes CLI lines
through `route_cli_line_to_phone` before the desktop parses them and tells phones when storage
is ready; `transport_state.rs` closes a workspace's phone sockets with its CLI;
`maintenance.rs` never unloads a workspace a phone has open. Android: `MainActivity` intent
filter (`singleTask`) → `NativeBridge.nativeSubmitPairLink` → `remote::submit_pair_link`.

**Verified (2026-10-15, Pixel 7a over Wi-Fi to a Windows desktop):**

- Enabling the switch listens on the LAN address; the QR code renders; "Copy link" works.
- Opening the link on the phone pairs it (the desktop shows the phone's name); the phone lists
  the desktop's projects and sessions, including sessions running on the desktop.
- Opening a session shows its live transcript (markdown and tables), with no error banners.
- Restarting the desktop: the listener comes back on its own and the phone reconnects with its
  stored pairing, without a new link.
- A new task sent from the phone runs on the desktop's CLI and the reply streams back.
- The model picker on a cold workspace lists the desktop's full catalog.
- Tests: `src/remote/*_tests.rs` (scope, real-TLS listener/client on loopback, gateway routing
  in `AppState`) and `mobile_shell_tests.rs` (phone hides the dock and terminal).

**Not in this slice:** the cargo feature split, a chosen (rather than detected) interface,
resume-from-seq, CJK fonts, the idle repaint, a "reconnecting" banner inside the chat page
(the status shows only in the sidebar footer), and a new draft's model label ("model…" when the
workspace default doesn't match the catalog, shared with the desktop). Socket IO uses blocking reads with a 15 ms
timeout so one thread can both read and write.

## Phone attachments (2026-10-31)

A path on the phone means nothing to the agent on the desktop, and the CLI's
`v4/attachment/*` upload needs a session id, which a new-task draft does not have yet. So the
phone always uploads attachments to the **desktop**:

- **Phone side:**
  - The composer's "+" calls `MainActivity.launchOpenDocument()`. It is registered through
    `remote::phone_files::set_file_picker`.
  - Keyboard-inserted images arrive through `ImeHost.commitContent`, which accepts `image/*`
    (Gboard's clipboard and image panels).
  - `PickedFiles.kt` reads the bytes (at most 20 MiB) and passes them through
    `NativeBridge.nativeAttachFile`.
  - The app polls `phone_files` alongside pairing links. `start_file_upload` sends images
    through the clipboard-image path, so they get the same format conversion, and uploads
    other files as `UploadKind::File`.
- **Transport:** the same begin/chunk/commit/abort state machine and shapes as
  `v4/attachment/*` (progress, retry, cancel, 16 uploads / 64 MiB), under `gcode/stage/`,
  with `UploadReceipt.desktop_stage` set. `sessionId` is empty in a draft.
- **Desktop side:**
  - `remote::staging::StagedUploads` sits on each open phone socket. The gateway answers
    `gcode/stage/*` itself, so the request never reaches the CLI.
  - It checks the sizes and the chunk order, and verifies the sha256 at commit.
  - It then writes a desktop paste temp file (keeping a safe extension) and returns its
    absolute path as `ref`.
  - The phone then sends that path as a plain attachment reference, exactly like a desktop
    paste.
- **Cleanup:** staged files are not deleted when the phone removes the chip, because the
  path may already have been sent. The paste-directory sweep reclaims them after 24 h.
- **Version requirement:** an older desktop refuses `gcode/stage/*` as "not available from
  the phone", so the phone and the desktop must both run this build.
- **Tests:** `remote/staging_tests.rs` and `backend/attachment_stage_tests.rs`, which covers
  a phone draft uploading end to end through `StagedUploads`.

## Phone images (2026-10-31)

Transcript images whose bytes are inline in the frame already work on the phone. Two kinds
did not: images named by a **desktop path** (Markdown image embeds and Read-tool images) and
**sent attachments** (shown only as text chips).

```
phone  ImageRef::File(path) ──gcode/file/read (≤384 KiB chunks)──▶ desktop gateway answers itself
       │ first chunk gives mtime+size ─▶ <temp>/gcode-images/f-<sha(path)>-<mtime>-<size> hit? stop
any    ImageRef::Attachment ──v4/attachment/read (≤512 KiB chunks)──▶ CLI (authorises by session row)
       phone only: <temp>/gcode-images/a-<sha(session,ref)> checked before any request
both   bytes ─▶ image_preview::decode ─▶ the same thumbnail and full-window viewer as the desktop
```

- **Owner:** `transcript::tool_images` decides the source (`remote_source`); the in-memory
  `ToolImageCache` keeps one result per image for the session.
  `remote::desktop_images::fetch` drives the chunk loop over `AppState::raw_query`, whose
  pending entry owns a oneshot sender, so a dropped connection ends the wait.
- **Desktop endpoint `gcode/file/read`** (`remote::file_read`), answered off the UI thread:
  - params `{path, offset, limit}`; result `{dataBase64, totalBytes, mtimeMs, nextOffset|null}`;
  - only an absolute path with an image extension whose magic bytes match that format, at most
    20 MiB, regular files only. Everything else is refused.
- **Consistency:** the phone rejects an answer whose size or mtime differs from the first
  chunk, or whose offsets do not line up, so a file changed mid-read never shows as a mixed
  image. At most 60 chunks.
- **Cache:** the phone's temp dir (Android's app cache). Written through a `.part` rename;
  trimmed to 200 MB, oldest first, on the first write after start.
- **Attachments:** image attachments render as right-aligned thumbnails under the bubble, read
  by `previewRef ?? ref` on both desktop and phone; other files keep their chip.
- **Version requirement:** an older desktop refuses `gcode/file/read`; the phone then shows
  "Desktop GCode is too old to send this image".
- **Tests:** `remote/file_read_tests.rs`, `remote/desktop_images_tests.rs` (phone fetch then
  disk-cache hit after one request, attachment cache hit with no request, refusal message) and
  the `user_row` thumbnail test.

## Phone layout

Ported from `packages/ui/src/WebRemoteControlMobileShell.tsx`. It applies when the window is at
most 767 logical px wide (`max-width: 767px`), on any platform, so desktop users can try it by
narrowing the window.

- **Two full-screen pages** instead of the sidebar beside the chat:
  - **Home:** the sidebar at full width (new task, search, tasks, projects, settings).
  - **Chat:** a 44 px header with a back arrow and "Task chat", then the full-width workspace panel.
- **Owner:** `RootView.mobile_page` (UI-local, not persisted). It starts on Home.
- **Moves to Chat** only on an explicit user action: selecting a task row, "New task" (row, "+"
  or shortcut), or picking a session or new task in the command centre. Background session
  changes never switch pages.
- **Back** (the header arrow, or Android's back key, which arrives as key `"back"`) closes the
  topmost layer: first the command centre, then Settings, then Chat goes to Home. On Home it
  does nothing yet.
- Above 767 px the layout is unchanged; `mobile_page` is ignored, not reset.
- **Subagent icons** (2026-11-01). The phone has no Agents dock, so the chat header's right side
  shows the active task's subagents, one bot icon each. Before this, an agent could only be opened
  from its card in the transcript.
  - **Which:** the parent task's running agents (`subagents.running` joined with `backgroundWorks`)
    plus its ended agents from the `session/subagents` directory (the same cache as the desktop
    Ended agents list, fetched again when the subagent revision moves). While a child is open, the
    icons still describe its parent.
  - **Number:** each agent is numbered 1, 2, 3… by start time (then child session id), across
    running and ended. An agent keeps its number when it ends.
  - **Status dot:** accent while running, waiting or blocked; danger for failed or lost; none
    for success or cancelled, with a muted icon.
  - **Room:** at most 4 icons. The open child comes first, then running agents, then the most
    recently started ended ones, shown in number order. The rest go into a "+N" menu
    (number, title, status).
  - **Tap:** an icon opens that child read-only (`open_child_conversation`). Tapping the
    highlighted icon of the open child returns to the parent.
  - The phone scope allows the read-only `session/subagents` query for this.

## Spike results (2026-10-14, Pixel 7a, Android 17 / API 37, Mali-G710)

The spike workspace lives outside the repo (`%LOCALAPPDATA%\Android\spike`).

- **Platform layer.** `crates/gpui_android` from Zdroid
  (github.com/Dylanmurzello/zed-android-port @ `1755a0d4`, crate licence Apache-2.0) builds
  against our pinned zed `1a28cff` **without patching gpui**. The adaptations:
  - copy gpui's `queue.rs`, which upstream exports only for windows/linux/wasm;
  - drop five extra `PlatformWindow` hooks that exist only in Zdroid's patched gpui;
  - implement the newer trait items (`visibility`, `on_visibility_change`, `on_system_sleep`, `prevent_idle_sleep`, the `restart`/`on_quit` signatures);
  - follow `gpui_wgpu` API drift (`WgpuSurfaceConfig`, `gpu_specs`, no `set_clear_color` / `load_fonts_dir`).
- **Other build fixes:**
  - `ely-gpui-component` depends on `gpui_platform`, which needs a `target_os = "android"` branch (patched copy).
  - ely's `rust-embed` assets need the `debug-embed` feature in debug builds.
  - `android-activity` 0.6.1 requires `androidx.games:games-activity:4.4.0`.
- **Rendering.** wgpu selects Mali-G710 on Vulkan and passes its configuration test. ely
  fonts, components, SVG icons and theme render correctly. Vsync is about 16.7 ms with
  about 0.05 ms per paint, at a logical viewport of 411×914 at 2.625×.
- **Touch.** Taps arrive as `MouseDown`/`MouseUp` with correct logical coordinates, and
  ely `Button::on_click` fires.
- **Soft keyboard.** A clean-room Kotlin IME host (1×1 `ImeHostView` with an
  `InputConnection` forwarding to the `gpui_android` JNI bridge, plus a surrounding-text
  mirror) works with Gboard. Commit, backspace (`KEYCODE_DEL`), auto-capitalisation and
  suggestions all work in ely's `TextInput`, which uses the same `EntityInputHandler` path
  as the composer.
- **Known gaps:**
  - CJK text renders as boxes: the system Noto CJK fonts must be registered via `add_fonts` or a `gpui_wgpu` API.
  - The window does not resize for the keyboard under edge-to-edge, so the keyboard inset must be passed to the UI (fixed, see below).
  - The app repaints every vsync even when idle (battery).
  - The JNI symbols are fixed to `com.zdroid.NativeBridge`; rename them to our own package (done: `dev.zcode.gpui.mobile`).
  - Zdroid's Kotlin app code has no declared licence (GPL repository), so it is **not** reused.

## Android app status (2026-10-14)

`apps/zcode-gpui-android` runs the real `zcode-gpui` window on the Pixel 7a as a remote
client of a paired desktop (see "LAN slice"). Before pairing it shows no workspaces and a
banner explaining how to pair.

- **Startup.** `android_main` sets `HOME` and `TMPDIR` to the app's private directories and calls
  `zcode_gpui::load_startup` and `open_window`, the same calls the desktop binary makes.
  Settings and data live under the app's private storage.
- **System bars and keyboard.** GameActivity draws edge-to-edge. `MainActivity` opts back out
  (`setDecorFitsSystemWindows(true)` with `adjustResize`, `targetSdk 34`), so Android sizes the
  surface between the status bar and the navigation bar or keyboard. gpui gets an ordinary
  resize. Targeting SDK 35+ removes this opt-out, and the UI would then have to pad for
  insets itself.
- **Keyboard re-show.** A tap that leaves a text input focused while the keyboard is hidden
  shows it again, as native Android text fields do (`gpui_android` `window.rs`).
- **Phone layout** (see "Phone layout"): task list and chat as two pages, with back via the
  header arrow or the Android back key. The first back press closes the keyboard, as usual on
  Android.
- **Verified on the phone:**
  - Home, chat and Settings pages fit the screen.
  - The composer is above the keyboard.
  - Gboard text is committed to the composer.
  - Back returns from chat to home.
- **Known gaps** (phone UI phase):
  - A tapped row keeps its hover highlight, because touch leaves gpui's hover state set.
  - Short labels wrap mid-word in Settings' narrow form column ("Languag e").
  - Error banners clip their text at the left edge.
  - Gboard auto-capitalisation in the composer is wrong ("hI"), which suggests the composer's
    surrounding text isn't reaching the IME mirror.
  - Hover-only controls have no touch equivalent.
  - CJK fonts are not registered yet.
  - The app repaints every vsync even when idle.
