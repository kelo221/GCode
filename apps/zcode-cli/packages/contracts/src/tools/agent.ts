// ============================================================
// Agent Tool - Subagent orchestration tool
// ============================================================
// 支持基于配置的子代理和异步启动。

import { z } from "zod";
import type { ToolCallId, TraceId } from "../interfaces/shared.js";
import type { ModelUsage } from "../model/index.js";
import { toToolJsonSchema } from "./json-schema.js";

export const AgentType = {
  GeneralPurpose: "general-purpose",
  Explore: "Explore",
} as const;

export type AgentType = string;

export const AgentInputSchema = z.object({
  description: z.string().describe("A short (3-5 word) description of the task"),
  prompt: z.string().describe("The task for the agent to perform"),
  subagent_type: z
    .string()
    .optional()
    .describe("The type of specialized agent to use for this task"),
  // subagent 模型由 Settings / Markdown profile 统一决定；若把调用级
  // model 暴露给父模型，历史 tool call 会持续生成旧 override 并覆盖当前配置。
  run_in_background: z
    .boolean()
    .optional()
    .describe(
      "Set to true to run this agent in the background. You will be notified when it completes.",
    ),
  isolation: z
    .enum(["worktree"])
    .optional()
    .describe(
      'Set to "worktree" to run the agent in its own git worktree (a jj workspace in jj repos) on branch/bookmark zcode/<agentId>, starting from your last commit (your uncommitted changes are not included). Its work is committed to that branch/bookmark; integrate it yourself afterwards.',
    ),
});

export type AgentInput = z.infer<typeof AgentInputSchema>;

export const AgentInputJsonSchema = toToolJsonSchema(AgentInputSchema);

export interface AgentTextContentBlock {
  type: "text";
  text: string;
}

/**
 * 子 agent 结束时用这个块列出没做完的条目；runner 把它从正文剥离成 `incomplete`。
 * Compact 续接提醒与 subagent 系统提示都引用同一个标签名。
 */
export const SUBAGENT_REMAINING_ITEMS_TAG = "remaining_items";

/**
 * `totalTokens` 是每次请求 input+output 的累计和（每一步都重新计入整段上下文），
 * 父模型曾把它读成"预算已耗尽"。这里给出能说明上下文真实状况的字段。
 */
export interface AgentContextReport {
  cumulativeBilledTokens?: number;
  peakContextTokens?: number;
  contextWindow?: number;
  compactionCount: number;
  stopReason?: string;
}

export interface AgentIncompleteReport {
  remainingItems: string[];
}

export interface AgentCompletedOutput {
  status: "completed";
  agentId: string;
  agentType: AgentType;
  description: string;
  prompt: string;
  content: AgentTextContentBlock[];
  totalToolUseCount: number;
  totalDurationMs: number;
  totalTokens?: number;
  usage?: ModelUsage;
  contextReport?: AgentContextReport;
  incomplete?: AgentIncompleteReport;
}

export interface AgentUsageEntriesInput {
  contextReport?: AgentContextReport;
  durationMs?: number;
  toolUseCount?: number;
  totalTokens?: number;
}

/** 同步 Agent 结果与后台 task-notification 共用的模型可见 usage 字段（按顺序）。 */
export function formatAgentUsageEntries(input: AgentUsageEntriesInput): [string, string][] {
  const report = input.contextReport;
  const cumulative = report?.cumulativeBilledTokens ?? input.totalTokens;
  const entries: [string, string][] = [];
  if (cumulative !== undefined) {
    entries.push([
      "cumulative_billed_tokens",
      `${cumulative} (sum of every model request; each request re-counts the whole context, so this is not a budget)`,
    ]);
  }
  if (report?.peakContextTokens !== undefined) {
    entries.push(["peak_context_tokens", String(report.peakContextTokens)]);
  }
  if (report?.contextWindow !== undefined) {
    entries.push(["context_window", String(report.contextWindow)]);
  }
  if (report) entries.push(["compactions", String(report.compactionCount)]);
  if (report?.stopReason) entries.push(["stop_reason", report.stopReason]);
  if (input.toolUseCount !== undefined) entries.push(["tool_uses", String(input.toolUseCount)]);
  if (input.durationMs !== undefined) entries.push(["duration_ms", String(input.durationMs)]);
  return entries;
}

/** 父模型看到的未完成说明；同步结果与后台通知共用。 */
export function formatAgentIncompleteNotice(
  agentId: string,
  incomplete: AgentIncompleteReport,
): string {
  return [
    `status: incomplete — the agent stopped with ${incomplete.remainingItems.length} item(s) unfinished:`,
    ...incomplete.remainingItems.map((item) => `- ${item}`),
    `To finish them, use SendMessage with to: '${agentId}' (it keeps its context); do not start a fresh agent for the same items.`,
  ].join("\n");
}

export interface AgentBackgroundedOutput {
  status: "async_launched";
  isAsync: true;
  agentId: string;
  agentType: AgentType;
  description: string;
  prompt: string;
  childSessionId: string;
  backgroundTaskId: string;
  outputFile: string;
  canReadOutputFile: boolean;
}

export type AgentOutput = AgentCompletedOutput | AgentBackgroundedOutput;

export const AgentTextContentBlockSchema = z
  .object({
    type: z.literal("text"),
    text: z.string(),
  })
  .strict();

export const AgentCompletedOutputSchema = z
  .object({
    status: z.literal("completed"),
    agentId: z.string(),
    agentType: z.string(),
    description: z.string(),
    prompt: z.string(),
    content: z.array(AgentTextContentBlockSchema),
    totalToolUseCount: z.number().int().nonnegative(),
    totalDurationMs: z.number().int().nonnegative(),
    totalTokens: z.number().int().nonnegative().optional(),
    usage: z.record(z.unknown()).optional(),
    contextReport: z
      .object({
        cumulativeBilledTokens: z.number().int().nonnegative().optional(),
        peakContextTokens: z.number().int().nonnegative().optional(),
        contextWindow: z.number().int().nonnegative().optional(),
        compactionCount: z.number().int().nonnegative(),
        stopReason: z.string().optional(),
      })
      .strict()
      .optional(),
    incomplete: z
      .object({ remainingItems: z.array(z.string()) })
      .strict()
      .optional(),
  })
  .strict();

export const AgentBackgroundedOutputSchema = z
  .object({
    status: z.literal("async_launched"),
    isAsync: z.literal(true),
    agentId: z.string(),
    agentType: z.string(),
    description: z.string(),
    prompt: z.string(),
    childSessionId: z.string(),
    backgroundTaskId: z.string(),
    outputFile: z.string(),
    canReadOutputFile: z.boolean(),
  })
  .strict();

export const AgentOutputSchema = z.union([
  AgentCompletedOutputSchema,
  AgentBackgroundedOutputSchema,
]);

export const AgentOutputJsonSchema = toToolJsonSchema(AgentOutputSchema);

/**
 * 同步子代理开始运行后失败时，runner 交给 Agent 工具、只给模型的交接数据。
 * 挂在抛出的错误上且不可枚举，不放进 `context`：日志序列化 `context`，UI 摘要读 cause 链，
 * 两边都不会带上部分报告。
 */
export const AgentFailureHandbackSchema = z
  .object({
    agentId: z.string(),
    report: z.string().optional(),
    totalToolUseCount: z.number().int().nonnegative(),
    totalDurationMs: z.number().int().nonnegative(),
    totalTokens: z.number().int().nonnegative().optional(),
  })
  .strict();

export type AgentFailureHandback = z.infer<typeof AgentFailureHandbackSchema>;

const AGENT_FAILURE_HANDBACK_PROPERTY = "agentFailureHandback";

/**
 * 在原错误上挂交接数据，返回同一个对象。
 * 修复原因：曾用 `Object.create` + `getOwnPropertyDescriptors` 复制错误再挂数据。Node 的 `stack`
 * 是读原对象内部槽的 accessor，复制品上读到 undefined，`tool_call_error` 与结果里的 stack 随之丢失。
 * 现在只加一个不可枚举属性，不改任何已有字段，错误身份与 stack 不变；对象不可扩展时不挂。
 */
export function withAgentFailureHandback<T extends Error>(
  error: T,
  handback: AgentFailureHandback,
): T {
  Reflect.defineProperty(error, AGENT_FAILURE_HANDBACK_PROPERTY, {
    configurable: true,
    enumerable: false,
    value: handback,
  });
  return error;
}

export function readAgentFailureHandback(error: unknown): AgentFailureHandback | undefined {
  if (typeof error !== "object" || error === null) return undefined;
  const parsed = AgentFailureHandbackSchema.safeParse(
    (error as Record<string, unknown>)[AGENT_FAILURE_HANDBACK_PROPERTY],
  );
  return parsed.success ? parsed.data : undefined;
}

export interface AgentToolCall {
  id: ToolCallId;
  name: "Agent";
  input: AgentInput;
  traceId: TraceId;
  startedAt: Date;
}

export interface AgentToolResult {
  toolCallId: ToolCallId;
  output: AgentOutput;
  traceId: TraceId;
  durationMs: number;
}

export const AgentErrorCode = {
  SUBAGENT_UNAVAILABLE: "agent_subagent_unavailable",
  BACKGROUND_UNAVAILABLE: "agent_background_unavailable",
  UNKNOWN_AGENT_TYPE: "agent_unknown_type",
  CHILD_RUNTIME_FAILED: "agent_child_runtime_failed",
} as const;

export type AgentErrorCode = (typeof AgentErrorCode)[keyof typeof AgentErrorCode];
