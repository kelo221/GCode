// ============================================================
// Nested tool calls (codemode)
// ============================================================
// codemode 脚本里的 `tools.<name>(args)` 经这里进入**同一条** executeToolCall 管线：
// deferred 解析、校验、PreToolUse hook、权限与审批、超时取消、secrets 脱敏、结果预算、
// PostToolUse hook 全部照常。见 docs/design/v2/tool/21-codemode.md。
//
// 嵌套调用不发 ToolCallScheduled，事件带 `source: "codemode"` + `parentToolCallId`：
// V4 projection 不为它们建行，ToolPart 只由 turn loop 落盘，所以它们不进模型历史。

import {
  CODEMODE_TOOL_NAME,
  SessionEventType,
  type ModelMessageContent,
  type SessionEvent,
} from "@zcode/contracts";
import { redactSecretText } from "../../secrets/secret-vault.js";
import type { ToolEntry, ToolExecutionResult } from "../types.js";
import type { BackgroundTaskTracker } from "./background-tasks.js";
import type { ToolExecuteOptions, ToolExecutorDeps } from "./types.js";

export const NESTED_TOOL_EVENT_SOURCE = "codemode";

/**
 * 脚本里不能调用的工具：自身（不允许嵌套脚本）、子 Agent / 消息、需要用户当场作答或切换模式的、
 * 控制轮次的，以及工具发现入口（脚本用 searchTools/describeTool）。`stopTurnOnSuccess` 的工具另按元数据排除。
 * 修复原因：Speak 没有 `stopTurnOnSuccess`（结束轮次由 speak_closing 另行判断），旧版只靠元数据排除，
 * 脚本实际能调 Speak，与工具说明和 spec 不符；朗读应由模型直接调用，这里显式列出。
 */
const NESTED_DISALLOWED_TOOLS: ReadonlySet<string> = new Set([
  CODEMODE_TOOL_NAME,
  "Agent",
  "Task",
  "SendMessage",
  "AskUserQuestion",
  "EnterPlanMode",
  "ExitPlanMode",
  "Compact",
  "Speak",
  "ToolSearch",
  "LocalToolSearch",
  "DeferredToolCall",
  "RespondToCoordinator",
  "submit_result",
  "escalate",
]);

export interface NestedToolDescriptor {
  name: string;
  description?: string;
  inputSchema?: unknown;
  /** 模型的工具列表里没有它（ToolSearch 延迟加载），脚本需要 describeTool 才知道参数。 */
  deferred: boolean;
}

export interface NestedToolPort {
  listTools(): NestedToolDescriptor[];
  /** 成功返回脚本可见的值；失败、拒绝、无效调用抛 Error（message 已脱敏）。 */
  execute(name: string, input: unknown, signal: AbortSignal): Promise<unknown>;
  /** 嵌套调用的 hook 附加上下文，由 codemode 拼进自己的结果。 */
  readonly hookContexts: readonly string[];
}

type ExecuteToolCall = (
  deps: ToolExecutorDeps,
  backgroundTasks: BackgroundTaskTracker,
  toolCall: { id: string; name: string; input: unknown },
  options?: ToolExecuteOptions,
) => Promise<ToolExecutionResult>;

export function isNestedToolAllowed(entry: ToolEntry | undefined): entry is ToolEntry {
  if (!entry) return false;
  const { metadata } = entry;
  return (
    !NESTED_DISALLOWED_TOOLS.has(metadata.name) &&
    metadata.stopTurnOnSuccess !== true &&
    metadata.providerVisible !== false
  );
}

function canRunInParallel(entry: ToolEntry): boolean {
  const { metadata } = entry;
  if (metadata.concurrentSafe) return true;
  return metadata.readOnly && !metadata.destructive;
}

export function createNestedToolPort(input: {
  deps: ToolExecutorDeps;
  backgroundTasks: BackgroundTaskTracker;
  parentToolCallId: string;
  options: ToolExecuteOptions | undefined;
  parentSignal: AbortSignal;
  executeToolCall: ExecuteToolCall;
}): NestedToolPort {
  const { deps, backgroundTasks, parentToolCallId, options, parentSignal } = input;
  const hookContexts: string[] = [];
  let counter = 0;
  // 非并发安全的调用在本脚本内串行，保持 scheduler 的副作用约定；并发安全的调用受 maxConcurrency 限制。
  let serialTail: Promise<unknown> = Promise.resolve();
  let running = 0;
  const waiters: Array<() => void> = [];
  // 修复原因：排队中的调用原先不监听取消，轮次中止后仍会拿到槽位再进入 executeToolCall；
  // 现在等待随取消 reject 并移出队列。
  const acquireSlot = async (signal: AbortSignal): Promise<() => void> => {
    while (running >= Math.max(1, deps.maxConcurrency)) {
      signal.throwIfAborted();
      await new Promise<void>((resolve, reject) => {
        const wake = () => {
          signal.removeEventListener("abort", onAbort);
          resolve();
        };
        const onAbort = () => {
          const index = waiters.indexOf(wake);
          if (index >= 0) waiters.splice(index, 1);
          reject(signal.reason);
        };
        signal.addEventListener("abort", onAbort, { once: true });
        waiters.push(wake);
      });
    }
    signal.throwIfAborted();
    running += 1;
    return () => {
      running -= 1;
      waiters.shift()?.();
    };
  };

  const nestedIds = new Set<string>();
  const nestedDeps: ToolExecutorDeps = {
    ...deps,
    emitEvent: (event) => deps.emitEvent(tagNestedToolEvent(event, nestedIds, parentToolCallId)),
  };

  const deferredNames = new Set(
    options?.toolSearch?.enabled
      ? options.toolSearch.catalog.filter((tool) => tool.deferred).map((tool) => tool.name)
      : [],
  );

  return {
    hookContexts,
    listTools() {
      return deps.registry
        .toContracts()
        .filter((contract) => isNestedToolAllowed(deps.registry.get(contract.name)))
        .map((contract) => ({
          name: contract.name,
          description: contract.description,
          inputSchema: contract.inputSchema,
          deferred: deferredNames.has(contract.name),
        }));
    },
    async execute(name, toolInput, signal) {
      const entry = deps.registry.get(name);
      if (!isNestedToolAllowed(entry)) {
        throw new Error(
          entry
            ? `${name} cannot be called from codemode.`
            : `No such tool available: ${name}. Use searchTools() to find tools.`,
        );
      }
      counter += 1;
      const id = `${parentToolCallId}~cm${counter}`;
      nestedIds.add(id);
      const callSignal = AbortSignal.any([parentSignal, signal]);
      const run = async (): Promise<ToolExecutionResult> => {
        // 串行队列里排在后面的调用：前一个结束时可能已经取消，不再启动。
        const release = await acquireSlot(callSignal);
        try {
          return await input.executeToolCall(
            nestedDeps,
            backgroundTasks,
            { id, name: entry.metadata.name, input: toolInput },
            { ...options, signal: callSignal },
          );
        } finally {
          release();
        }
      };
      let pending: Promise<ToolExecutionResult>;
      if (canRunInParallel(entry)) {
        pending = run();
      } else {
        pending = serialTail.then(run, run);
        serialTail = pending.catch(() => undefined);
      }
      const result = await pending;
      if (result.hookAdditionalContexts) hookContexts.push(...result.hookAdditionalContexts);
      return toScriptValue(result);
    },
  };
}

function tagNestedToolEvent(
  event: SessionEvent,
  nestedIds: ReadonlySet<string>,
  parentToolCallId: string,
): SessionEvent {
  switch (event.type) {
    case SessionEventType.ToolCallStarted:
    case SessionEventType.ToolCallProgress:
    case SessionEventType.ToolCallResult:
    case SessionEventType.ToolCallError:
      break;
    default:
      return event;
  }
  const payload = event.payload as unknown as Record<string, unknown>;
  if (!nestedIds.has(String(payload.toolCallId))) return event;
  return {
    ...event,
    payload: {
      ...payload,
      source: NESTED_TOOL_EVENT_SOURCE,
      parentToolCallId,
    } as unknown as SessionEvent["payload"],
  };
}

/** 工具结果 → 脚本值。用模型可见内容（已脱敏、已按预算截断），而不是 handler 原始输出。 */
export function toScriptValue(result: ToolExecutionResult): unknown {
  if (!result.success) {
    throw new Error(redactSecretText(result.error?.message ?? `${result.toolName} failed`));
  }
  const structured = readStructuredContent(result.output);
  if (structured !== undefined) return structured;
  return contentToScriptValue(result.modelContent ?? result.serialization?.content ?? "");
}

function readStructuredContent(output: unknown): unknown {
  if (!output || typeof output !== "object") return undefined;
  const structured = (output as { structuredContent?: unknown }).structuredContent;
  if (!structured || typeof structured !== "object") return undefined;
  return Object.keys(structured).length > 0 ? structured : undefined;
}

function contentToScriptValue(content: ModelMessageContent): unknown {
  if (typeof content === "string") return content;
  const items: unknown[] = [];
  for (const block of content) {
    if (block.type === "text") items.push({ type: "text", text: block.text });
    else if (block.type === "image") {
      const match = /^data:([^;,]+);base64,(.*)$/s.exec(block.dataUrl);
      if (match) items.push({ type: "image", mimeType: match[1], data: match[2] });
    }
  }
  if (items.every((item) => (item as { type: string }).type === "text")) {
    return items.map((item) => (item as { text: string }).text).join("\n");
  }
  return items;
}
