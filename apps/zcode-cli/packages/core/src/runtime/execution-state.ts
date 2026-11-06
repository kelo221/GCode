import { resolveExecutionState, type ExecutionState } from "@zcode/shared";
import {
  SESSION_ENTRY_EXECUTION_STATE,
  SessionEventType,
  traceContextToLogContext,
  type TraceContext,
  type SessionId,
  type SessionEntryInfo,
} from "@zcode/contracts";
import type { AgentRuntimeInternal } from "./internal.js";
import {
  unpublishedPermissionGrants,
  recoverPendingPermissionGrant,
} from "./permission-grant-recovery.js";

type ExecutionStateListener = (state: ExecutionState) => Promise<void>;

/**
 * 父 runtime 执行状态变更的订阅者（每个存活的 subagent child 一个）。
 * 修复原因：child 只在 spawn 时复制一次父模式，父会话之后切到「完全访问」不会传给运行中的
 * child，子代理仍按 build 逐次请求批准（docs/subagent-permission-mode-follow.md）。
 */
const executionStateListeners = new WeakMap<AgentRuntimeInternal, Set<ExecutionStateListener>>();

export function onRuntimeExecutionStateChanged(
  runtime: AgentRuntimeInternal,
  listener: ExecutionStateListener,
): () => void {
  let listeners = executionStateListeners.get(runtime);
  if (!listeners) {
    listeners = new Set();
    executionStateListeners.set(runtime, listeners);
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function readRuntimeExecutionState(runtime: AgentRuntimeInternal): ExecutionState {
  return resolveExecutionState(runtime.config);
}

export async function persistExecutionState(
  runtime: AgentRuntimeInternal,
  state = readRuntimeExecutionState(runtime),
): Promise<void> {
  if (!runtime.sessionPersisted || !runtime.sessionStore?.saveSessionEntry) return;
  await runtime.sessionStore.saveSessionEntry(buildExecutionStateEntry(runtime.sessionId, state));
}

export function buildExecutionStateEntry(
  sessionId: SessionId,
  state: ExecutionState,
): SessionEntryInfo {
  const timestamp = Date.now();
  return {
    id: `${sessionId}:runtime-execution-state`,
    sessionID: sessionId,
    type: SESSION_ENTRY_EXECUTION_STATE,
    touchSession: false,
    time: { created: timestamp, updated: timestamp },
    data: state,
  };
}

/** 权限与 Plan 是一个已消费状态；保存失败不发布成功快照，也不提前改内存。 */
export async function applyRuntimeExecutionState(
  runtime: AgentRuntimeInternal,
  input: { mode?: string; planEnabled?: boolean },
  cause: { source: "command" | "tool"; toolCallId?: string; traceContext?: TraceContext },
): Promise<ExecutionState> {
  if (runtime.permissionFullAccessPending)
    throw new Error("Permission update is busy; retry mode change");
  if (unpublishedPermissionGrants.has(runtime)) await recoverPendingPermissionGrant(runtime);
  const previous = readRuntimeExecutionState(runtime);
  const next = resolveExecutionState(input, previous);
  if (next.mode === previous.mode && next.planEnabled === previous.planEnabled) return next;
  if (next.planEnabled && !previous.planEnabled) {
    const goal = await runtime.readSessionTargetForContext?.(
      cause.traceContext ?? runtime.rootTraceContext,
    );
    if (goal?.status === "active")
      throw new Error("Plan and Goal cannot be active at the same time.");
  }
  await persistExecutionState(runtime, next);
  runtime.config.mode = next.mode;
  runtime.config.planEnabled = next.planEnabled;
  if (previous.planEnabled !== next.planEnabled)
    runtime.needsPlanModeExitReminder = !next.planEnabled;
  const trace = cause.traceContext ?? runtime.rootTraceContext;
  await runtime.appendEvent(
    runtime.createEvent(
      SessionEventType.SessionModeChanged,
      {
        ...next,
        previousMode: previous.mode,
        previousPlanEnabled: previous.planEnabled,
        source: cause.source,
        ...(cause.toolCallId ? { toolCallId: cause.toolCallId } : {}),
      },
      trace,
    ),
    trace,
  );
  // 父状态已提交并发布后再通知 child；单个 child 跟随失败不回滚父会话的模式切换。
  for (const listener of executionStateListeners.get(runtime) ?? []) {
    try {
      await listener(next);
    } catch (error) {
      runtime.logger?.warn("Subagent execution state follow failed", {
        ...traceContextToLogContext(trace),
        errorMessage: error instanceof Error ? error.message : String(error),
        event: "subagent.execution_state.follow_failed",
        module: "core.runtime",
        status: "failed",
      });
    }
  }
  return next;
}
