# Backend lifecycle log

GPUI keeps an on-disk record of every agent backend child it spawns so that an
unexplained restart can be diagnosed after the fact.

## Why

2026-11-07: a backend died at 12:25:24 with no `shutdown` entry in the CLI
jsonl log, no Windows Error Reporting event and no low-memory event. Its stderr
(where a Bun panic or a fatal JS error would land) only went to GPUI's in-memory
log panel, and its exit code was never recorded, so the cause could not be
recovered.

## Rules

- File: `<home>/.zcode/v2/gpui-logs/gpui-backend-YYYY-MM-DD.log` (local date;
  `<home>` honours profile isolation). Plain text, one line per entry:
  `YYYY-MM-DD HH:MM:SS.mmm [pid N] <text>`.
- Entries, all written from the connection's background threads (never the UI
  thread):
  - `spawned workspace=<path> via=<launch description>` once per child. No
    environment variables or arguments (they may carry secrets).
  - every child stderr line verbatim, also after GPUI has stopped listening to
    the connection, so dying words are kept.
  - `exited code=<n> (0x<hex>) uptime=<d> <who>` when the process is reaped,
    where `<who>` is `killed by GPUI` if GPUI invoked the connection's kill
    (idle unload, overflow restart, shutdown) and
    `not requested by GPUI` otherwise (crash, external kill, self exit).
    On Unix a signal is shown as `signal=<n>`.
- Lines are written unbuffered (one append per line) so they survive a GPUI
  crash.
- Size cap 16 MiB per file: when reached, one `size cap reached` note is
  written and further stderr lines are dropped for that day; `spawned` and
  `exited` lines are always written.
- Retention: on the first write of a GPUI process, `gpui-backend-*.log` files
  older than 14 days are deleted.
- Logging failures (unwritable directory etc.) are silent and never affect the
  connection. Unit tests never write the file.

## Acceptance

- Start GPUI, open a project: a `spawned` line appears with the backend pid.
- Kill that `zcode-backend.exe` in Task Manager: an `exited ... not requested
  by GPUI` line appears, followed by a new `spawned` line from the auto-restart.
- Let a project idle-unload: its `exited` line says `killed by GPUI`.
