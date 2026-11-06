# Subagent permission mode follows the parent

Status: implemented 2026-11-06 (GCode fork).

## Problem

A session showed "Full access" while its subagents kept asking before every Bash, Edit and
Write. The logs for that session (`tool.permission.resolved`, `ruleId: mode.build.*`) showed
three separate defects:

1. A child runtime copied the parent's execution state once, at spawn. Switching the parent to
   Full access later never reached children that were already running.
2. Resuming an existing child (`resumeFromStore`, e.g. "continue" on a stopped background
   agent) restored the child's own persisted execution state (`build`) and ignored the parent.
3. GPUI: picking a permission mode on a running session while plan was on only wrote a draft
   override for the next message. The selector already showed the new mode, the backend kept the
   old one, and an agent-driven ExitPlanMode dropped the draft (`reconcile_plan_override`).

## Rules

- The parent runtime is the only owner of the execution state its subagents run with. A child's
  state is always derived as
  `resolveSubagentPermissionMode(parentPlan ? "plan" : parentMode, profile.permissionMode, builtInExplore)`,
  i.e. an explicit profile `permissionMode` (acceptEdits / plan / auto / bypassPermissions) and
  the built-in Explore yolo default keep their meaning, and parent `guarded` still tightens a
  derived yolo.
- Derived at three moments: spawn, resume of an existing child, and every committed parent
  execution-state change while the child runtime is alive. The child commits it through the same
  `applyRuntimeExecutionState` path (persisted entry + `SessionModeChanged`), so a later cold
  resume and the child's own nested children see the same value.
- A follow failure (e.g. the child is mid permission-grant) is logged as `warn` and does not
  fail the parent's mode change.
- `switchCollaborationMode` accepts an optional `planEnabled`. GPUI sends
  `{ mode, planEnabled: true }` when the user picks a permission mode while plan is on, so the
  backend changes the base mode immediately and plan stays on. No draft override is created.

## Event order

```text
user picks Full access
  → GPUI switchCollaborationMode {mode: yolo[, planEnabled: true]}
  → session-facade.setMode → parent applyRuntimeExecutionState (persist, SessionModeChanged)
  → parent execution-state listeners (one per live child)
      → derive child state → child applyRuntimeExecutionState (persist, SessionModeChanged)
          → grandchild listeners (same rule, one level down)
```

## Acceptance

- Core test: a running general-purpose child created under `build` resolves a later tool call
  as `yolo` after the parent switches; a child with `permissionMode: acceptEdits` stays `edit`.
- Core test: resuming an existing child whose persisted state is `build` under a `yolo` parent
  runs as `yolo`.
- Protocol/handler test: `switchCollaborationMode {mode: "yolo", planEnabled: true}` keeps plan
  on and sets the base mode.
- GPUI test: selecting a permission mode with plan on in a live session sends the command with
  `planEnabled: true` and leaves no draft override.
