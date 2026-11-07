# Sending to a sleeping agent

GPUI unloads a project's agent after it has been idle (`ZCODE_GPUI_IDLE_SECS`,
see `app/maintenance.rs`). Sending a message must wake it without the user
having to resend.

## Why

2026-11-07 (BACKEND_LOG.md caught it): the ZCode project's agent was unloaded
at 12:51, the user sent a message at 16:57. The first send only spawned the
agent and restored the draft ("backend still starting — try again"); the next
reached the fresh agent before the open conversation was resubscribed, so the
CLI inbox answered `proto.sessionNotFound` (`getRevision` is null for a session
the host has not loaded). Nothing resubscribed the open conversation, so sends
kept failing until the user reopened it.

## Rules

- Owner: `AppState.waking_send` (one slot), `composer/waking_send.rs`.
- A composer send (not a held-queue decision) whose workspace agent is not
  ready is parked instead of failing. "Not ready" means:
  - the agent is spawning/starting (`pumping` but not `started`), or
  - the target is an existing conversation that is not `subscribed`.
  The composer is cleared; a strip above it shows a spinner, "Waking the
  agent…" and a Cancel button.
- The park advances after every connection event of that workspace:
  started → subscribe the conversation (once) → when `subscribed`, run the
  normal send. A new chat (no session yet) sends as soon as the agent started.
- A second send while parked for the same target is appended to the parked
  text (attachments too); a send to another target is refused with a log line.
- Give up and put the text/attachments back into the draft (the composer if
  the user is still there, otherwise that conversation's saved draft), with an
  error banner, when: the agent reports `backend down` / `no backend`, the
  subscription answer arrives without making the conversation subscribed, or
  the user clicks Cancel. No timer: the agent's own restart limit ends a
  stuck start.
- The park belongs to `(workspace, session)`. If the user is no longer on that
  target when it would send, or sends from another target, the parked message
  goes back into its own conversation's saved draft instead of being sent
  behind the user's back. The strip only shows while viewing the target.

## Acceptance

- Unit tests (`composer/waking_send_tests.rs`): parked while starting, sends
  after start+subscribe; new chat sends right after start; failed subscribe
  and Cancel restore the draft; agent down restores the draft.
- Live: with `ZCODE_GPUI_IDLE_SECS=30`, open a conversation, wait for the
  agent to unload, send one message: spinner strip, then the message is sent
  once without resending.
