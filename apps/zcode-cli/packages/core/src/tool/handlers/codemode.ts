// ============================================================
// codemode Tool Handler
// ============================================================
// 模型写一段 JavaScript，在 QuickJS 沙箱里经 `tools.<name>(args)` 调用其他工具，只有脚本输出
// 回到模型。嵌套调用走完整执行管线（executor/nested-tools.ts）。
// 设计见 docs/design/v2/tool/21-codemode.md；沙箱来自 pi（@earendil-works/pi-codemode，MIT）。

import {
  CodemodeSourceError,
  parseCodemodeSource,
  type CodemodeCall,
  type CodemodeOutputItem,
  type CodemodeResult,
  type CodemodeStoreWrites,
  type CodemodeTool,
} from "@earendil-works/pi-codemode";
import { z } from "zod";
import {
  CODEMODE_DISPLAY_MAX_CALLS,
  CODEMODE_TOOL_NAME,
  type CodemodeToolResultDisplayPayload,
  type JsonSchema,
  type ModelMessageContent,
  type ModelMessageContentBlock,
} from "@zcode/contracts";
import { describeNestedTool, searchNestedTools, shortDescription } from "../codemode/catalog.js";
import { runCodemodeScript } from "../codemode/sandbox.js";
import type { ToolEntry, ToolHandler, ToolHandlerFailure } from "../types.js";

const DEFAULT_MAX_OUTPUT_TOKENS = 10_000;
const CHARS_PER_TOKEN = 4;
const MAX_OUTPUT_IMAGES = 4;
const CODEMODE_ERROR_CODE = { INVALID_SOURCE: 1, UNAVAILABLE: 2 } as const;

export const CodemodeInputSchema = z.object({ code: z.string().min(1) }).strict();

// 运行时校验 handler 输出；calls/storeWrites 的内部形状由 pi-codemode 定义，这里只钉外层。
const CodemodeOutputSchema = z.object({
  ok: z.boolean(),
  text: z.string(),
  images: z.array(z.object({ mimeType: z.string(), data: z.string() })),
  calls: z.array(z.object({ name: z.string(), status: z.string() }).passthrough()),
  storeWrites: z
    .object({ set: z.record(z.string(), z.unknown()), delete: z.array(z.string()) })
    .optional(),
  durationMs: z.number(),
});

const CodemodeInputJsonSchema: JsonSchema = {
  type: "object",
  properties: {
    code: {
      type: "string",
      description:
        'Raw JavaScript (not JSON, not in a markdown fence). Runs as the body of an async function. Optional first line: // @options: {"max_output_tokens": N, "timeout_ms": N}',
    },
  },
  required: ["code"],
  additionalProperties: false,
};

export interface CodemodeOutput {
  ok: boolean;
  /** 已格式化的文本结果（含 console 与错误）。 */
  text: string;
  images: Array<{ mimeType: string; data: string }>;
  calls: CodemodeCall[];
  storeWrites?: CodemodeStoreWrites;
  durationMs: number;
}

export function isCodemodeOutput(value: unknown): value is CodemodeOutput {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as CodemodeOutput).text === "string" &&
    Array.isArray((value as CodemodeOutput).calls) &&
    Array.isArray((value as CodemodeOutput).images)
  );
}

const CODEMODE_DESCRIPTION =
  "Run a JavaScript script that calls your other tools. Only what the script outputs comes back to you, so use it to batch several tool calls, run them in parallel, loop over results, and cut large results down before you see them.";

const CODEMODE_INSTRUCTIONS = [
  "The input is raw JavaScript run as the body of an async function in a sandbox: top-level await and return work. There are no Node APIs, file system, network, timers, require or import; the outside world is reachable only through tools.",
  "Call a tool as `await tools.<Name>(args)` with the same arguments you would give it directly (characters invalid in identifiers become `_`, e.g. `tools.mcp__my_server__search`). Each call goes through the normal permission checks; a denied, failed or invalid call rejects with an Error. Use Promise.allSettled() to keep the calls that succeeded.",
  "Tool results come back as the text you would normally see (MCP tools with structured content return that object; images come back as image blocks). Calls are real and are not rolled back if the script fails.",
  "Output: `text(value)` adds a text item (non-strings as JSON), `image(block)` adds an image, `console.log/info/warn/error` add console lines, `return value` adds a final item, `exit()` ends the script successfully.",
  "Discovery: `ALL_TOOLS` lists every callable tool; `await searchTools(query, {limit})` ranks tools (including ones not loaded into your tool list); `await describeTool(name)` returns a tool's TypeScript declaration.",
  "`store(key, value)` / `load(key)` keep small JSON values (ids, cursors) across codemode calls in this conversation; values persist only when the script succeeds.",
  'Optional first line: `// @options: {"max_output_tokens": 2000, "timeout_ms": 60000}`. Output beyond max_output_tokens (default 10000) is trimmed to its start and end. timeout_ms has no default.',
  "Not callable from scripts: codemode itself, Agent, SendMessage, AskUserQuestion, plan-mode tools, Compact, Speak and tool-search tools. Use codemode for multi-step or bulk tool work; for a single tool call, call the tool directly.",
];

function formatDuration(ms: number): string {
  return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)}s`;
}

function stringifyValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

/** pi 的结果格式：状态行、文本项（多项带编号头）、console 块、失败时的错误。 */
export function formatCodemodeText(
  result: CodemodeResult,
  durationMs: number,
  maxOutputTokens: number,
): string {
  const texts: string[] = [];
  const consoleLines: string[] = [];
  for (const item of result.output) {
    if (item.type !== "text") continue;
    if (item.console) consoleLines.push(item.text);
    else texts.push(item.text);
  }
  if (result.ok && result.value !== undefined) texts.push(stringifyValue(result.value));
  const parts = [`Script ${result.ok ? "completed" : "failed"} in ${formatDuration(durationMs)}`];
  if (texts.length === 1) parts.push(texts[0] as string);
  else
    texts.forEach((text, index) =>
      parts.push(`==> text ${index + 1}/${texts.length} <==\n${text}`),
    );
  if (consoleLines.length > 0) {
    parts.push(`<console_output>\n${consoleLines.join("\n")}\n</console_output>`);
  }
  if (!result.ok) {
    const { error } = result;
    const label = error.kind === "script" ? (error.name ?? "Error") : error.kind;
    parts.push(`Script error: ${label}: ${error.message}`);
  }
  return trimToBudget(parts.join("\n\n"), maxOutputTokens * CHARS_PER_TOKEN);
}

export function trimToBudget(text: string, maxChars: number): string {
  if (text.length <= maxChars) return text;
  const half = Math.floor(maxChars / 2);
  const omitted = text.length - half * 2;
  const banner = `[... ${omitted} characters omitted; print less or raise max_output_tokens ...]`;
  // 修复原因：`max_output_tokens: 0` 时 half 为 0，而 `text.slice(-0)` 等于整段文本，截断被绕过。
  if (half === 0) return banner;
  return `${text.slice(0, half)}\n\n${banner}\n\n${text.slice(-half)}`;
}

function collectImages(output: readonly CodemodeOutputItem[]): CodemodeOutput["images"] {
  return output
    .filter((item): item is Extract<CodemodeOutputItem, { type: "image" }> => item.type === "image")
    .slice(0, MAX_OUTPUT_IMAGES)
    .map((item) => ({ mimeType: item.mimeType, data: item.data }));
}

const codemodeHandler: ToolHandler = async (input, context) => {
  const { code } = CodemodeInputSchema.parse(input);
  const nested = context.nestedTools;
  if (!nested) {
    return {
      result: false,
      errorCode: CODEMODE_ERROR_CODE.UNAVAILABLE,
      message: "codemode_unavailable: this session cannot run nested tool calls.",
    } satisfies ToolHandlerFailure;
  }
  let parsed: ReturnType<typeof parseCodemodeSource>;
  try {
    parsed = parseCodemodeSource(code);
  } catch (error) {
    if (!(error instanceof CodemodeSourceError)) throw error;
    return {
      result: false,
      errorCode: CODEMODE_ERROR_CODE.INVALID_SOURCE,
      message: `invalid_source: ${error.message}`,
    } satisfies ToolHandlerFailure;
  }

  const catalog = nested.listTools();
  const tools: CodemodeTool[] = catalog.map((tool) => ({
    name: tool.name,
    ...(shortDescription(tool.description)
      ? { description: shortDescription(tool.description) }
      : {}),
    execute: (args, { signal }) => nested.execute(tool.name, args, signal),
  }));
  const globals: CodemodeTool[] = [
    {
      name: "searchTools",
      spread: true,
      execute: (args) => {
        const [query, options] = args as [unknown, { limit?: number; namespace?: string }?];
        return searchNestedTools(catalog, String(query ?? ""), options ?? {});
      },
    },
    {
      name: "describeTool",
      execute: (name) => describeNestedTool(catalog, String(name)),
    },
  ];

  const store = context.codemodeStore;
  const startedAt = Date.now();
  const result = await runCodemodeScript({
    code: parsed.code,
    tools,
    globals,
    options: {
      signal: context.abortSignal,
      ...(parsed.options.timeoutMs !== undefined ? { timeoutMs: parsed.options.timeoutMs } : {}),
      ...(store ? { store: Object.fromEntries(store) } : {}),
    },
  });
  const durationMs = Date.now() - startedAt;
  let text = formatCodemodeText(
    result,
    durationMs,
    parsed.options.maxOutputTokens ?? DEFAULT_MAX_OUTPUT_TOKENS,
  );
  if (nested.hookContexts.length > 0) text = `${text}\n\n${nested.hookContexts.join("\n")}`;
  const storeWrites =
    result.ok &&
    (Object.keys(result.storeWrites.set).length > 0 || result.storeWrites.delete.length > 0)
      ? result.storeWrites
      : undefined;
  if (store && storeWrites) applyCodemodeStoreWrites(store, storeWrites);
  return {
    ok: result.ok,
    text,
    images: collectImages(result.output),
    calls: result.calls,
    ...(storeWrites ? { storeWrites } : {}),
    durationMs,
  } satisfies CodemodeOutput;
};

export function applyCodemodeStoreWrites(
  store: Map<string, unknown>,
  writes: CodemodeStoreWrites,
): void {
  for (const key of writes.delete) store.delete(key);
  for (const [key, value] of Object.entries(writes.set)) store.set(key, value);
}

function formatCodemodeModelContent(output: unknown): ModelMessageContent {
  if (!isCodemodeOutput(output)) return "codemode returned an invalid result.";
  if (output.images.length === 0) return output.text;
  const blocks: ModelMessageContentBlock[] = [{ type: "text", text: output.text }];
  for (const image of output.images) {
    blocks.push({
      type: "image",
      mediaType: image.mimeType,
      dataUrl: `data:${image.mimeType};base64,${image.data}`,
    });
  }
  return blocks;
}

/** GPUI 工具行用的有界调用清单。 */
export function createCodemodeDisplay(
  output: unknown,
): CodemodeToolResultDisplayPayload | undefined {
  if (!isCodemodeOutput(output)) return undefined;
  return {
    kind: "codemode",
    ok: output.ok,
    calls: output.calls.slice(0, CODEMODE_DISPLAY_MAX_CALLS).map((call) => ({
      // display schema 要求非空名字；空名会让整块 display 在协议层被丢掉。
      name: call.name.slice(0, 256) || "unknown",
      status: call.status,
      durationMs: Math.max(0, Math.round(call.durationMs)),
    })),
    ...(output.calls.length > CODEMODE_DISPLAY_MAX_CALLS ? { truncated: true as const } : {}),
  };
}

export const codemodeToolEntry: ToolEntry = {
  capability: "Run a JavaScript script that calls other tools",
  nestedToolAccess: true,
  metadata: {
    name: CODEMODE_TOOL_NAME,
    description: CODEMODE_DESCRIPTION,
    modelInstructions: CODEMODE_INSTRUCTIONS,
    readOnly: false,
    destructive: false,
    // 同一会话的脚本共享 store，并且各自的嵌套调用会互相交错：按顺序执行。
    concurrentSafe: false,
    allowedInPlanMode: true,
    // 修复依据：codemode 本身不碰外部世界，每个嵌套调用都在执行管线里单独判权限（plan mode 下的写操作
    // 同样被拒）。按 `session` 档声明，脚本本身不额外弹审批，也能在 plan mode 下运行只读脚本。
    sideEffectScope: "session",
    riskLevel: "low",
    needsApproval: false,
  },
  handler: codemodeHandler,
  inputSchema: CodemodeInputJsonSchema,
  outputSchema: {
    type: "object",
    properties: {
      ok: { type: "boolean" },
      text: { type: "string" },
      images: { type: "array" },
      calls: { type: "array" },
      storeWrites: { type: "object" },
      durationMs: { type: "number" },
    },
    required: ["ok", "text", "images", "calls", "durationMs"],
  },
  runtimeInputSchema: CodemodeInputSchema,
  runtimeOutputSchema: CodemodeOutputSchema,
  formatModelContent: formatCodemodeModelContent,
  // 文本已按 max_output_tokens 限长，图片数量有上限。
  isModelContentBounded: () => true,
  permission: {
    permission: "codemode",
    reason: "codemode runs a sandboxed script; each tool it calls is checked on its own",
    riskLevel: "low",
    sideEffectScope: "session",
    needsApproval: false,
    patternSources: ["toolName"],
    alwaysAllowPatternSources: ["toolName"],
    denyPriority: "beforeAsk",
  },
  resultBudget: {
    maxInlineBytes: 100_000,
    maxModelBytes: 1_000_000,
    strategy: "inline",
  },
  timeout: { kind: "none" },
  cancellation: {
    supported: true,
    cleanup: "bestEffort",
    userVisibleMessage: "codemode script was cancelled",
  },
  trace: {
    required: true,
    propagateToAdapters: false,
    recordInput: "summary",
    recordOutput: "summary",
  },
};
