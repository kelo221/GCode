import {
  SUBAGENT_REMAINING_ITEMS_TAG,
  SessionEventType,
  dropTrailingHighSurrogate,
  getModelUsageContextTokens,
  type AgentContextReport,
  type AgentIncompleteReport,
  type ModelUsage,
  type SessionEvent,
  type SubagentResultMeta,
  type SubagentStopOptions,
} from "@zcode/contracts";
import { formatTaskNotification } from "../runtime-task/notification.js";

const REMAINING_ITEMS_BLOCK_PATTERN = new RegExp(
  `<${SUBAGENT_REMAINING_ITEMS_TAG}>([\\s\\S]*?)(?:</${SUBAGENT_REMAINING_ITEMS_TAG}>|$)`,
  "g",
);

/**
 * 子 agent 在最终答复末尾用 `<remaining_items>` 列出没做完的条目。块从正文剥离，
 * 条目交给父 agent 作为 `incomplete`，父 agent 才知道应 SendMessage 续接而不是重开。
 */
export function extractSubagentRemainingItems(response: string): {
  incomplete?: AgentIncompleteReport;
  text: string;
} {
  const remainingItems: string[] = [];
  let sawBlock = false;
  const text = response.replace(REMAINING_ITEMS_BLOCK_PATTERN, (_match, body: string) => {
    sawBlock = true;
    for (const line of body.split(/\r?\n/)) {
      const item = line.replace(/^\s*(?:[-*•]|\d+[.)])\s*/, "").trim();
      if (item) remainingItems.push(item);
    }
    return "";
  });
  if (!sawBlock) return { text: response };
  return {
    text: text.trimEnd(),
    ...(remainingItems.length > 0 ? { incomplete: { remainingItems } } : {}),
  };
}

export function buildSubagentContextReport(input: {
  contextWindow?: number;
  cumulativeBilledTokens?: number;
  events: readonly SessionEvent[];
}): AgentContextReport {
  let compactionCount = 0;
  let peakContextTokens: number | undefined;
  let stopReason: string | undefined;
  for (const event of input.events) {
    if (event.type === SessionEventType.CompactBoundary) {
      compactionCount += 1;
      continue;
    }
    if (event.type !== SessionEventType.ModelComplete || !isRecord(event.payload)) continue;
    const payload = event.payload;
    // compact summary 请求和工具内部嵌套用量不代表 agent 自己的工作上下文。
    if (payload.querySource === "compact" || payload.stopReason === "tool_internal") continue;
    if (isRecord(payload.usage)) {
      const contextTokens = getModelUsageContextTokens(payload.usage as ModelUsage);
      if (contextTokens !== undefined) {
        peakContextTokens = Math.max(peakContextTokens ?? 0, contextTokens);
      }
    }
    if (typeof payload.stopReason === "string") stopReason = payload.stopReason;
  }
  return {
    ...(input.cumulativeBilledTokens === undefined
      ? {}
      : { cumulativeBilledTokens: input.cumulativeBilledTokens }),
    ...(peakContextTokens === undefined ? {} : { peakContextTokens }),
    ...(input.contextWindow === undefined || !Number.isFinite(input.contextWindow)
      ? {}
      : { contextWindow: Math.floor(input.contextWindow) }),
    compactionCount,
    ...(stopReason ? { stopReason } : {}),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export type LocalAgentTaskNotificationStatus =
  | "completed"
  | "failed"
  | "killed";

const UNKNOWN_ERROR = "Unknown error";
/** 结构化 error 的上限，与 workflow 行的 error 相同。 */
const SUBAGENT_RESULT_ERROR_MAX_CHARS = 2_000;

export interface LocalAgentTaskNotificationInput {
  agentId: string;
  agentType?: string;
  contextReport?: AgentContextReport;
  description: string;
  error?: string;
  incomplete?: AgentIncompleteReport;
  outputFile: string;
  parentToolCallId: string;
  result?: string;
  status: LocalAgentTaskNotificationStatus;
  /** killed 时的停止者；缺席表示系统停止。 */
  stoppedBy?: SubagentStopOptions["initiator"];
  totalDurationMs?: number;
  totalTokens?: number;
  totalToolUseCount?: number;
  usage?: ModelUsage;
}

export function formatLocalAgentTaskNotification(
  input: LocalAgentTaskNotificationInput,
): string {
  return formatTaskNotification({
    agentId: input.agentId,
    description: input.description,
    ...(input.incomplete ? { incomplete: input.incomplete } : {}),
    outputFile: input.outputFile,
    result: input.result,
    status: input.status,
    summary: `Agent "${input.description}" ${formatLocalAgentOutcome(input)}`,
    taskId: input.agentId,
    taskType: "local_agent",
    toolUseId: input.parentToolCallId,
    // 只有 completed 通知带 <usage>；failed / killed 不带。
    usage:
      input.status === "completed"
        ? {
            ...(input.contextReport ? { contextReport: input.contextReport } : {}),
            durationMs: input.totalDurationMs,
            modelUsage: input.usage,
            toolUseCount: input.totalToolUseCount,
            totalTokens: input.totalTokens,
          }
        : undefined,
  });
}

/**
 * 与通知文本同一份输入生成的结构化终态，随 originMeta 下发。
 * App 的“执行失败”hover 读这里的 error，不解析 summary 文本。
 */
export function localAgentSubagentResult(
  input: Pick<LocalAgentTaskNotificationInput, "error" | "status">,
): SubagentResultMeta {
  return input.status === "failed"
    ? { status: "failed", error: boundStructuredError(failureReason(input.error)) }
    : { status: input.status };
}

// 修复原因：结构化 error 随 originMeta 进入轮头和每一帧快照，原样复制超长的 provider 原文
// 会让快照和持久化 metadata 膨胀。只截这里；模型看到的 summary 仍是全文。
function boundStructuredError(error: string): string {
  if (error.length <= SUBAGENT_RESULT_ERROR_MAX_CHARS) return error;
  return `${dropTrailingHighSurrogate(error.slice(0, SUBAGENT_RESULT_ERROR_MAX_CHARS - 1))}…`;
}

// 修复原因：用 `||` 兜底时，只有空白的 error 不会变成 Unknown error，
// summary 会以 "failed: " 加空白结尾，hover 也是空白。
function failureReason(error: string | undefined): string {
  return error?.trim() ? error : UNKNOWN_ERROR;
}

// summary 措辞。失败原因直接跟在 "failed: " 后面，与 localAgentSubagentResult 的 error 相同。
function formatLocalAgentOutcome(
  input: Pick<LocalAgentTaskNotificationInput, "error" | "status" | "stoppedBy">,
): string {
  if (input.status === "completed") return "finished";
  if (input.status === "failed") return `failed: ${failureReason(input.error)}`;
  // TaskStop 是模型自己停的。
  if (input.stoppedBy === "model") return "was stopped by ZCode";
  if (input.stoppedBy === "user") return "was stopped by user";
  return "was stopped";
}
