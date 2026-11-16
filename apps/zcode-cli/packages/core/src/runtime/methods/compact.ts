import {
  CompactPhase,
  CompactReason,
  CompactTrigger,
  CoreErrorType,
  DEFAULT_COMPACT_CONTEXT_WINDOW,
  SessionEventType,
  createMessageId,
  createModelUsageSummaryFromEvents,
  createPartId,
  runWithContextAsync,
  traceContextToLogContext,
  estimateMessageTokens,
  shouldAutoCompact,
} from "../deps.js";
import type {
  AutoCompactPolicyConfig,
  AutoCompactTokenOverride,
  SessionEvent,
  TraceContext,
  TurnId,
} from "../deps.js";
import type { RuntimeMessageEntry } from "../../agent/message-history.js";
import {
  throwIfTurnAborted,
  createTurnFailureError,
  isTurnCancellationError,
  appendTurnOutcomeEvent,
  buildRuntimeProviderRequestMessages,
  hasEnoughRuntimeEntriesToCompact,
} from "../helpers/index.js";
import type {
  CompactTimelineContext,
  TurnResult,
  RunModelTextRequestOptions,
} from "../types.js";
import type { Model } from "../deps.js";
import type { ProviderContextUsageSnapshot } from "../types.js";
import type { AgentRuntimeInternal } from "../internal.js";
import { autoCompactDecisionLogContext } from "./compact-log-context.js";
import { resolveNormalRequestMaxOutputTokens } from "./model-token-limits.js";
import type {
  AutoCompactLoopContext,
  AutoCompactOutcome,
  CompactAttemptOutcome,
  ReactiveCompactLoopContext,
} from "./turn-loop-state.js";
import { recordTurnUsageFact } from "./usage-observability.js";
import { findLatestCommittedAssistantUsage, latestContextTokens } from "./turn-model-step-usage.js";
import { rollOverTotalTokensBudget } from "../helpers/total-tokens-budget.js";

/**
 * 手动 compact 失败时保证恰好一个终态 lifecycle 事件（按 sourceCommandId 关联）。
 * 原因：started 之后、重试循环 try 之前（MCP 初始化、工具暴露）抛错，或 started 之前
 * （会话持久化、选择、Stop）抛错时，都不会写 CompactFailed；客户端只能用超时猜测结束。
 * 已有终态时不重复写；已有 started 时复用同一 operation，让 running marker 收口。
 */
async function settleManualCompactTimeline(
  runtime: AgentRuntimeInternal,
  options: {
    abortSignal?: AbortSignal;
    error: unknown;
    events: SessionEvent[];
    inputId?: string;
    startedAt: number;
    traceContext: TraceContext;
  },
): Promise<void> {
  const settled = options.events.some(
    (event) =>
      event.type === SessionEventType.CompactCompleted ||
      event.type === SessionEventType.CompactFailed,
  );
  if (settled) return;
  const started = [...options.events]
    .reverse()
    .find((event) => event.type === SessionEventType.CompactStarted)?.payload as
    | Partial<CompactTimelineContext>
    | undefined;
  const timeline: CompactTimelineContext = {
    operationId: started?.operationId ?? `cmp_${crypto.randomUUID()}`,
    messageId: started?.messageId ?? createMessageId(),
    partId: started?.partId ?? createPartId(),
    trigger: started?.trigger ?? CompactTrigger.Manual,
    phase: started?.phase ?? CompactPhase.StandaloneTurn,
    compactReason: started?.compactReason ?? CompactReason.UserRequested,
    ...((started?.sourceCommandId ?? options.inputId)
      ? { sourceCommandId: started?.sourceCommandId ?? options.inputId }
      : {}),
    startedAt: started?.startedAt ?? options.startedAt,
    ...(started?.preCompactTokenCount !== undefined
      ? { preCompactTokenCount: started.preCompactTokenCount }
      : {}),
  };
  await runtime.finishCompactTimelineFailure({
    abortSignal: options.abortSignal,
    error: options.error,
    events: options.events,
    timeline,
    traceContext: options.traceContext,
  });
}

export async function executeManualCompact(
  this: AgentRuntimeInternal,
  input: string,
  customInstructions: string | undefined,
  turnId: TurnId,
  turnTraceContext: TraceContext,
  abortSignal?: AbortSignal,
  inputId?: string,
  model?: Model,
): Promise<TurnResult> {
  const events: SessionEvent[] = [];
  const startedAt = Date.now();
  const activeTurn = this.beginActiveTurn(turnId, turnTraceContext, "compact", false);
  return runWithContextAsync(turnTraceContext, async () => {
    this.logger?.info("Compact started", {
      ...traceContextToLogContext(turnTraceContext),
      event: "compact.started",
      inputLength: input.length,
      module: "core.runtime",
      status: "started",
    });

    await this.ensureSessionPersisted(input, turnTraceContext);

    const turnStartedEvent = this.createEvent(
      SessionEventType.TurnStarted,
      {
        turnNumber: this.turnNumber,
        input,
        inputId,
        // 手动 /compact 是维护命令，不是用户真实 query。
        // 事件仍保留 raw input 便于恢复/排查，但 v4 投影不能把它渲染成 user bubble。
        inputVisibility: "model-only",
      },
      turnTraceContext,
    );
    await this.appendEvent(turnStartedEvent, turnTraceContext);
    events.push(turnStartedEvent);

    try {
      throwIfTurnAborted(abortSignal);
      const compactResult = await this.compactActiveConversation(
        customInstructions,
        turnTraceContext,
        events,
        {
          abortSignal,
          compactReason: CompactReason.UserRequested,
          phase: CompactPhase.StandaloneTurn,
          ...(inputId ? { sourceCommandId: inputId } : {}),
          ...(model ? { model } : {}),
        },
      );
      throwIfTurnAborted(abortSignal);

      const turnUsage = createModelUsageSummaryFromEvents(events);
      const completeEvent = this.createEvent(
        SessionEventType.TurnComplete,
        {
          response: compactResult.displayText,
          tokenCount: compactResult.tokenCount,
          usage: turnUsage,
          toolCallCount: 0,
          historyRoundCount: 1,
          duration: Date.now() - startedAt,
          resultType: "success",
          cacheStats: this.messageHistory.getCacheStats(),
          inputId,
        },
        turnTraceContext,
      );
      await this.appendEvent(completeEvent, turnTraceContext);
      events.push(completeEvent);
      await recordTurnUsageFact(this, {
        completedAt: Date.now(),
        events,
        startedAt,
        status: "completed",
        traceContext: turnTraceContext,
        turnId,
      });

      this.turnNumber++;
      const projection = await this.rebuildProjection();
      this.logger?.info("Compact completed", {
        ...traceContextToLogContext(turnTraceContext),
        durationMs: Date.now() - startedAt,
        event: "compact.completed",
        module: "core.runtime",
        status: "completed",
      });

      return {
        response: compactResult.displayText,
        turnId,
        traceId: turnTraceContext.traceId,
        usage: turnUsage,
        events,
        projection,
      };
    } catch (error) {
      const coreError = createTurnFailureError(error, abortSignal, "Compact failed");
      const preserveQueueAutoDrainOnCancel =
        coreError.type === CoreErrorType.TurnCancelled &&
        this.activeForegroundExecution?.preserveQueueAutoDrainOnCancel === true;
      if (coreError.type === CoreErrorType.TurnCancelled && !preserveQueueAutoDrainOnCancel) {
        // Stop compact 与普通 Stop 语义一致：队列重新暂停，外层 FIFO 恢复窗口同时关闭。
        this.queueAutoDrain = false;
        this.queueExternalDrainActive = false;
      }
      await settleManualCompactTimeline(this, {
        abortSignal,
        error,
        events,
        inputId,
        startedAt,
        traceContext: turnTraceContext,
      });
      await appendTurnOutcomeEvent(this, {
        coreError,
        events,
        durationMs: Date.now() - startedAt,
        turnPhase: "compact",
        inputId,
        traceContext: turnTraceContext,
        fallbackMessage: "Compact failed",
        logEvent: "compact.failed",
        logLabel: "Compact",
        preserveQueueAutoDrainOnCancel,
      });
      await recordTurnUsageFact(this, {
        completedAt: Date.now(),
        error: coreError,
        events,
        startedAt,
        status: coreError.type === CoreErrorType.TurnCancelled ? "cancelled" : "error",
        traceContext: turnTraceContext,
        turnId,
      });

      throw coreError;
    }
  }).finally(() => {
    this.finishActiveTurn(activeTurn);
  });
}

export async function autoCompactIfNeeded(
  this: AgentRuntimeInternal,
  turnTraceContext: TraceContext,
  events: SessionEvent[],
  abortSignal: AbortSignal | undefined,
  context: AutoCompactLoopContext,
): Promise<AutoCompactOutcome> {
  throwIfTurnAborted(abortSignal);

  const config: AutoCompactPolicyConfig = {
    contextWindow: context.model.properties.contextWindow,
    ...this.config.compact,
    maxOutputTokens: resolveNormalRequestMaxOutputTokens({
      modelMaxOutputTokens: context.model.optionSpecs.maxOutputTokens.max,
    }),
    modelContextBudgetStrategy: this.config.modelContextBudgetStrategy,
  };
  const activeEntries = context.turnRequestState.entries;
  const activeProjection = buildRuntimeProviderRequestMessages(this, {
    entries: activeEntries,
    applyCacheControl: false,
    model: context.model,
  });
  const { messages: activeMessages, sourceEntries } = activeProjection;
  const tokenOverride = buildProviderUsageTokenOverride(activeMessages, sourceEntries);
  const decision = shouldAutoCompact({
    messages: activeMessages,
    config,
    consecutiveFailures: this.autoCompactConsecutiveFailures,
    tokenOverride,
    // 修复原因同 reactive compact：按投影后的消息判断时，被删的回复不算历史，可总结的部分没有 assistant 就不压缩。
    hasEnoughHistory: hasEnoughRuntimeEntriesToCompact(activeEntries),
  });

  // Agent 用 Compact 工具明确请求时不看阈值；历史不够长仍由 compactActiveConversation 返回 skipped。
  const requested = context.requested;
  if (!requested && !decision.shouldCompact) {
    this.logger?.debug("Auto compact skipped", {
      ...traceContextToLogContext(turnTraceContext),
      event: "compact.auto.skipped",
      module: "core.runtime",
      compactReason: context.compactReason,
      modelStepIndex: context.modelStepIndex,
      phase: context.phase,
      reason: decision.reason,
      ...autoCompactDecisionLogContext(decision),
    });
    return "skipped";
  }

  if (!requested && context.rapidRefill.shouldBlock) {
    this.logger?.warn("Autocompact rapid-refill breaker tripped", {
      ...traceContextToLogContext(turnTraceContext),
      event: "compact.rapid_refill_breaker",
      compactReason: context.compactReason,
      consecutiveRapidRefills: context.rapidRefill.consecutiveRapidRefills,
      modelStepIndex: context.modelStepIndex,
      module: "core.runtime",
      phase: context.phase,
      status: "failed",
      toolTurnsSinceCompact: context.rapidRefill.toolTurnsSinceCompact,
      trigger: CompactTrigger.Auto,
      ...autoCompactDecisionLogContext(decision),
    });
    return "rapid_refill_blocked";
  }

  // 跳过压缩时无需构建 listing 工具合同；读取失败仍向上抛出，不计入压缩失败。
  const agentListingTools = context.getAgentListingTools?.();
  this.logger?.info("Auto compact started", {
    ...traceContextToLogContext(turnTraceContext),
    event: "compact.auto.started",
    compactReason: context.compactReason,
    modelStepIndex: context.modelStepIndex,
    module: "core.runtime",
    phase: context.phase,
    requested: requested !== undefined,
    ...autoCompactDecisionLogContext(decision),
  });

  try {
    const compactResult = await this.compactActiveConversation(
      requested?.instructions,
      turnTraceContext,
      events,
      {
        abortSignal,
        compactContextTelemetry: {
          inputTokens: decision.tokenCount,
          policyContextWindowTokens: decision.contextWindow,
          thresholdTokens: decision.threshold,
          tokenSource: decision.tokenSource,
        },
        autoCompactThreshold: decision.threshold,
        compactReason: context.compactReason,
        phase: context.phase,
        trigger: CompactTrigger.Auto,
        agentListingTools,
        activeEntries,
        getContinuationTools: context.getContinuationTools,
        ...(context.model ? { model: context.model } : {}),
      },
    );
    if (compactResult.outcome === "skipped") {
      return "skipped";
    }
    context.turnRequestState.entries = compactResult.entries;
    rollOverTotalTokensBudgetAfterCompact(this, activeEntries);
    this.autoCompactConsecutiveFailures = 0;
    this.logger?.info("Auto compact completed", {
      ...traceContextToLogContext(turnTraceContext),
      event: "compact.auto.completed",
      compactReason: context.compactReason,
      modelStepIndex: context.modelStepIndex,
      module: "core.runtime",
      phase: context.phase,
      ...autoCompactDecisionLogContext(decision),
    });
    return "compacted";
  } catch (error) {
    if (isTurnCancellationError(error, abortSignal)) {
      throw error;
    }
    // 熔断计数只约束阈值触发的自动压缩；agent 请求失败不应让后续自动压缩提前停用。
    if (!requested) this.autoCompactConsecutiveFailures++;
    this.logger?.warn("Auto compact failed", {
      ...traceContextToLogContext(turnTraceContext),
      errorMessage: error instanceof Error ? error.message : String(error),
      event: "compact.auto.failed",
      failureCount: this.autoCompactConsecutiveFailures,
      compactReason: context.compactReason,
      modelStepIndex: context.modelStepIndex,
      module: "core.runtime",
      phase: context.phase,
      ...autoCompactDecisionLogContext(decision),
    });
    return "failed";
  }
}

/**
 * 主循环里的 auto / reactive compaction 提交后，把 compact 前的上下文加进预算计数，数值不会跳回；
 * 手动 compact 不调用，下一条真实用户消息会重新起算。
 */
function rollOverTotalTokensBudgetAfterCompact(
  runtime: AgentRuntimeInternal,
  compactedEntries: readonly RuntimeMessageEntry[],
): void {
  runtime.totalTokensBudget = rollOverTotalTokensBudget(
    runtime.totalTokensBudget,
    latestContextTokens(compactedEntries),
  );
}

function buildProviderUsageTokenOverride(
  messages: RunModelTextRequestOptions["messages"],
  sourceEntries: readonly (RuntimeMessageEntry | undefined)[],
): AutoCompactTokenOverride | undefined {
  const latestUsage = findLatestCommittedAssistantUsage(sourceEntries);
  if (!latestUsage || latestUsage.messageIndex >= messages.length) {
    return undefined;
  }

  const { baseline, messageIndex } = latestUsage;
  const incrementalStartIndex =
    baseline.contextUsageTokens === undefined ? messageIndex : messageIndex + 1;
  const incrementalTokenCount = estimateMessageTokens(messages.slice(incrementalStartIndex));
  // usage 归属于已提交 assistant，反向扫描可随 history replacement 自然移动，
  // 不再依赖可能失效的绝对 message cursor。若 output 是否存在已被历史归一化抹平，
  // 则 provider input 只覆盖 assistant 之前的请求，assistant 本身仍进入本地增量。
  const providerBaseTokenCount = baseline.contextUsageTokens ?? baseline.inputTokens;
  return {
    baseTokenCount: providerBaseTokenCount,
    cacheReadTokens: baseline.cacheReadTokens,
    cacheWriteTokens: baseline.cacheWriteTokens,
    contextUsageTokenCount: baseline.contextUsageTokens,
    incrementalTokenCount,
    outputTokens: baseline.outputTokens,
    source: "provider_usage",
    tokenCount: providerBaseTokenCount + incrementalTokenCount,
  };
}

export function estimateCurrentModelInputTokens(
  messages: RunModelTextRequestOptions["messages"],
  sourceEntries: readonly (RuntimeMessageEntry | undefined)[] = [],
): number {
  return (
    buildProviderUsageTokenOverride(messages, sourceEntries)?.tokenCount ??
    estimateMessageTokens(messages)
  );
}

export async function reactiveCompactAfterContextExceeded(
  this: AgentRuntimeInternal,
  originalError: unknown,
  turnTraceContext: TraceContext,
  events: SessionEvent[],
  abortSignal: AbortSignal | undefined,
  context: ReactiveCompactLoopContext,
): Promise<CompactAttemptOutcome> {
  throwIfTurnAborted(abortSignal);

  if (this.config.compact?.enabled === false) {
    this.logger?.warn("Reactive compact skipped because compact is disabled", {
      ...traceContextToLogContext(turnTraceContext),
      errorMessage: originalError instanceof Error ? originalError.message : String(originalError),
      event: "compact.reactive.skipped",
      modelStepIndex: context.modelStepIndex,
      module: "core.runtime",
      rapidRefillCount: context.rapidRefillCount,
      reason: "disabled",
    });
    return "skipped";
  }

  const activeEntries = context.activeEntries ?? context.turnRequestState.entries;
  const activeProjection = buildRuntimeProviderRequestMessages(this, {
    entries: activeEntries,
    applyCacheControl: false,
    model: context.model,
  });
  const { messages: activeMessages, sourceEntries } = activeProjection;
  // 入口按全部存储条目判断；compactActiveConversation 保留最后一轮后对其余部分再判断一次。
  // 修复原因：原来按投影后的消息判断；只有思考或空白的回复在投影第一步被删，可总结的部分没有 assistant
  // 时这里先跳过，用户直接看到超窗错误，而这些用户消息其实可以压缩。
  if (!hasEnoughRuntimeEntriesToCompact(activeEntries)) {
    this.logger?.warn("Reactive compact skipped because there is not enough history", {
      ...traceContextToLogContext(turnTraceContext),
      errorMessage: originalError instanceof Error ? originalError.message : String(originalError),
      event: "compact.reactive.skipped",
      messageCount: activeMessages.length,
      modelStepIndex: context?.modelStepIndex,
      module: "core.runtime",
      rapidRefillCount: context?.rapidRefillCount,
      reason: "not_enough_messages",
    });
    return "skipped";
  }

  const tokenOverride = buildProviderUsageTokenOverride(activeMessages, sourceEntries);
  const contextWindow = context.model.properties.contextWindow;

  this.logger?.warn("Reactive compact started after model context overflow", {
    ...traceContextToLogContext(turnTraceContext),
    errorMessage: originalError instanceof Error ? originalError.message : String(originalError),
    event: "compact.reactive.started",
    messageCount: activeMessages.length,
    modelStepIndex: context?.modelStepIndex,
    module: "core.runtime",
    rapidRefillCount: context?.rapidRefillCount,
    tokenCount: estimateMessageTokens(activeMessages),
  });

  try {
    const compactResult = await this.compactActiveConversation(
      undefined,
      turnTraceContext,
      events,
      {
        abortSignal,
        compactContextTelemetry: {
          inputTokens: tokenOverride?.tokenCount ?? estimateMessageTokens(activeMessages),
          policyContextWindowTokens:
            contextWindow !== undefined && Number.isFinite(contextWindow) && contextWindow > 0
              ? Math.floor(contextWindow)
              : DEFAULT_COMPACT_CONTEXT_WINDOW,
          tokenSource: tokenOverride?.source ?? "estimate",
        },
        compactReason: CompactReason.ProviderOverflow,
        initialPromptTooLongCause: originalError,
        phase: CompactPhase.Reactive,
        trigger: CompactTrigger.Reactive,
        agentListingTools: context.agentListingTools,
        activeEntries,
        getContinuationTools: context.getContinuationTools,
        model: context.model,
      },
    );
    if (compactResult.outcome === "skipped") {
      return "skipped";
    }
    context.turnRequestState.entries = compactResult.entries;
    rollOverTotalTokensBudgetAfterCompact(this, activeEntries);
    this.autoCompactConsecutiveFailures = 0;
    this.logger?.info("Reactive compact completed; retrying model request", {
      ...traceContextToLogContext(turnTraceContext),
      event: "compact.reactive.completed",
      modelStepIndex: context?.modelStepIndex,
      module: "core.runtime",
      rapidRefillCount: context?.rapidRefillCount,
    });
    return "compacted";
  } catch (error) {
    if (isTurnCancellationError(error, abortSignal)) {
      throw error;
    }
    this.autoCompactConsecutiveFailures++;
    this.logger?.warn("Reactive compact failed after model context overflow", {
      ...traceContextToLogContext(turnTraceContext),
      errorMessage: error instanceof Error ? error.message : String(error),
      event: "compact.reactive.failed",
      failureCount: this.autoCompactConsecutiveFailures,
      modelStepIndex: context?.modelStepIndex,
      module: "core.runtime",
      rapidRefillCount: context?.rapidRefillCount,
    });
    return "failed";
  }
}
