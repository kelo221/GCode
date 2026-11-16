// ============================================================
// codemode tool catalog: script-side discovery + "only" mode exposure
// ============================================================

import {
  renderDeclarations,
  renderToolSample,
  toCodemodeIdentifier,
  type CodemodeTool,
} from "@earendil-works/pi-codemode";
import type { ModelToolContract } from "@zcode/contracts";
import type { NestedToolDescriptor } from "../executor/nested-tools.js";

/** 约 3000 token（按 4 字符/token 估算）的内联声明预算。 */
export const CODEMODE_INLINE_DECLARATION_CHARS = 12_000;
const SEARCH_DEFAULT_LIMIT = 8;
const SHORT_DESCRIPTION_CHARS = 200;

export function shortDescription(description: string | undefined): string | undefined {
  if (!description) return undefined;
  const firstParagraph = description.split(/\n\s*\n/u)[0]?.trim() ?? "";
  return firstParagraph.length > SHORT_DESCRIPTION_CHARS
    ? `${firstParagraph.slice(0, SHORT_DESCRIPTION_CHARS - 1)}…`
    : firstParagraph || undefined;
}

function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/u)
    .filter((token) => token.length > 1);
}

/** 名字命中权重高于描述；简单词频打分，够用且不引入索引状态。 */
export function searchNestedTools(
  tools: readonly NestedToolDescriptor[],
  query: string,
  options: { limit?: number; namespace?: string } = {},
): Array<{ name: string; description?: string; deferred: boolean }> {
  const terms = tokenize(query);
  const limit = Math.max(1, Math.min(options.limit ?? SEARCH_DEFAULT_LIMIT, 50));
  const scored = tools
    .filter((tool) => !options.namespace || tool.name.startsWith(options.namespace))
    .map((tool) => {
      const nameTokens = tokenize(tool.name);
      const descriptionTokens = tokenize(tool.description ?? "");
      let score = 0;
      for (const term of terms) {
        if (tool.name.toLowerCase() === term) score += 10;
        score += nameTokens.filter((token) => token.includes(term)).length * 3;
        score += descriptionTokens.filter((token) => token === term).length;
      }
      return { tool, score };
    })
    .filter(({ score }) => terms.length === 0 || score > 0)
    .sort(
      (left, right) => right.score - left.score || left.tool.name.localeCompare(right.tool.name),
    )
    .slice(0, limit);
  return scored.map(({ tool }) => ({
    name: toCodemodeIdentifier(tool.name),
    ...(shortDescription(tool.description)
      ? { description: shortDescription(tool.description) }
      : {}),
    deferred: tool.deferred,
  }));
}

export function describeNestedTool(tools: readonly NestedToolDescriptor[], name: string): string {
  const tool = tools.find(
    (candidate) => candidate.name === name || toCodemodeIdentifier(candidate.name) === name,
  );
  if (!tool) throw new Error(`No such tool: ${name}. Use searchTools() to find tools.`);
  return renderToolSample(toCodemodeTool(tool));
}

export function toCodemodeTool(
  tool: Pick<NestedToolDescriptor, "name" | "description" | "inputSchema">,
): Pick<CodemodeTool, "name" | "description" | "inputSchema"> {
  return {
    name: tool.name,
    ...(tool.description ? { description: tool.description } : {}),
    ...(tool.inputSchema && typeof tool.inputSchema === "object"
      ? { inputSchema: tool.inputSchema as Record<string, unknown> }
      : {}),
  };
}

/**
 * `only` 模式：发给模型的工具只留 codemode 与轮次控制类工具，其他工具改由脚本调用。
 * codemode 的描述里内联被隐藏工具的 TypeScript 声明（按预算），超出预算的只列名字。
 */
export const CODEMODE_ONLY_MODE_KEPT_TOOLS: ReadonlySet<string> = new Set([
  "codemode",
  "Agent",
  "Task",
  "SendMessage",
  "ListAgents",
  "AskUserQuestion",
  "EnterPlanMode",
  "ExitPlanMode",
  "TodoWrite",
  "TodoRead",
  "Speak",
  "Compact",
  "RespondToCoordinator",
  "submit_result",
  "escalate",
  "Skill",
]);

export function applyCodemodeOnlyExposure(
  tools: readonly ModelToolContract[],
): ModelToolContract[] {
  const codemode = tools.find((tool) => tool.name === "codemode");
  if (!codemode) return [...tools];
  const hidden = tools.filter(
    (tool) =>
      !CODEMODE_ONLY_MODE_KEPT_TOOLS.has(tool.name) &&
      tool.name !== "ToolSearch" &&
      tool.name !== "LocalToolSearch" &&
      tool.name !== "DeferredToolCall",
  );
  const kept = tools.filter((tool) => CODEMODE_ONLY_MODE_KEPT_TOOLS.has(tool.name));
  const description = `${codemode.description ?? ""}\n\n${renderHiddenToolDeclarations(hidden)}`;
  return kept.map((tool) => (tool.name === "codemode" ? { ...tool, description } : tool));
}

function renderHiddenToolDeclarations(hidden: readonly ModelToolContract[]): string {
  const inline: Array<Pick<CodemodeTool, "name" | "description" | "inputSchema">> = [];
  const listedOnly: string[] = [];
  let used = 0;
  for (const tool of hidden) {
    const candidate = toCodemodeTool({
      name: tool.name,
      description: shortDescription(tool.description),
      inputSchema: tool.inputSchema,
    });
    const size = renderToolSample(candidate).length;
    if (used + size > CODEMODE_INLINE_DECLARATION_CHARS) {
      listedOnly.push(toCodemodeIdentifier(tool.name));
      continue;
    }
    used += size;
    inline.push(candidate);
  }
  const parts = [
    "Other tools are not offered directly in this session. Call them from a script:",
    "```ts",
    renderDeclarations({ tools: inline.map((tool) => ({ ...tool, execute: () => undefined })) }),
    "```",
  ];
  if (listedOnly.length > 0) {
    parts.push(
      `Also available (call describeTool(name) for the parameters): ${listedOnly.join(", ")}.`,
    );
  }
  return parts.join("\n");
}
