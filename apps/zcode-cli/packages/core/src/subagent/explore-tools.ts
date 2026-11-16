// 对齐文件搜索型子 agent 的工具面。direct branch 会暴露 Glob/Grep；
// embedded search branch 则通过 Bash find/grep 接管搜索。注意：白名单刻意不含任何
// 文件写工具（Write/Edit/ApplyPatch），因此 Bash 是唯一的副作用入口。Explore 继承父会话的权限模式与
// Plan 状态，Bash 是否只读由 shell AST 判定（非只读命令按父会话模式询问或拒绝），prompt 也禁止修改。
export const EXPLORE_AGENT_ALLOWED_TOOLS = [
  "ToolSearch",
  "DeferredToolCall",
  "Bash",
  "Glob",
  "Grep",
  "Read",
  "WebFetch",
  "WebSearch",
  "TodoWrite",
] as const;

// GPUI fff 索引搜索：只有注入了 CodeSearchPort 的会话才把它们加进 Explore 工具面，
// 其余宿主（Electron）的允许列表与事件载荷保持不变。
export const EXPLORE_AGENT_CODE_SEARCH_TOOLS = ["FindFiles", "SearchCode"] as const;

// codemode：父会话开启时 Explore 同享；脚本只能调用 Explore 自己 registry 里的工具。
export const EXPLORE_AGENT_CODEMODE_TOOLS = ["codemode"] as const;

const CODE_SEARCH_EQUIVALENTS: ReadonlyMap<string, string> = new Map([
  ["Glob", "FindFiles"],
  ["Grep", "SearchCode"],
]);

/**
 * 修复原因：自定义 agent profile 的 `tools:` 写的是 Glob/Grep，而 embedded search 分支会注销
 * 这两个工具，子代理就只剩 Bash find/grep，即便父会话有 fff 索引。只读搜索能力等价，
 * 所以按名单里的 Glob/Grep 补上 FindFiles/SearchCode；调用方只在父工具面确有这对工具时调用。
 */
export function withCodeSearchEquivalents(toolNames: readonly string[]): readonly string[] {
  const present = new Set(toolNames);
  const added = toolNames.flatMap((toolName) => {
    const equivalent = CODE_SEARCH_EQUIVALENTS.get(toolName);
    if (!equivalent || present.has(equivalent)) return [];
    present.add(equivalent);
    return [equivalent];
  });
  return added.length > 0 ? [...toolNames, ...added] : toolNames;
}

export type ExploreAgentAllowedTool =
  | (typeof EXPLORE_AGENT_ALLOWED_TOOLS)[number]
  | (typeof EXPLORE_AGENT_CODE_SEARCH_TOOLS)[number]
  | (typeof EXPLORE_AGENT_CODEMODE_TOOLS)[number];

export const EXPLORE_AGENT_EMBEDDED_SEARCH_ALLOWED_TOOLS = [
  "ToolSearch",
  "DeferredToolCall",
  "Bash",
  "Read",
  "WebFetch",
  "WebSearch",
  "TodoWrite",
] as const;

const EXPLORE_AGENT_DESCRIPTION_TOOL_PRIORITY = [
  "FindFiles",
  "SearchCode",
  "Glob",
  "Grep",
  "Read",
  "Bash",
  "WebFetch",
  "WebSearch",
  "TodoWrite",
] as const satisfies readonly ExploreAgentAllowedTool[];
const EXPLORE_AGENT_DESCRIPTION_TOOL_PRIORITY_SET = new Set<ExploreAgentAllowedTool>(
  EXPLORE_AGENT_DESCRIPTION_TOOL_PRIORITY,
);

export function buildExploreAllowedTools(
  options: {
    embeddedSearchEnabled?: boolean;
    codeSearch?: boolean;
    codemode?: boolean;
  } = {},
): readonly ExploreAgentAllowedTool[] {
  const base: readonly ExploreAgentAllowedTool[] = options.embeddedSearchEnabled
    ? EXPLORE_AGENT_EMBEDDED_SEARCH_ALLOWED_TOOLS
    : EXPLORE_AGENT_ALLOWED_TOOLS;
  const bash = base.indexOf("Bash") + 1;
  const tools = options.codeSearch
    ? [...base.slice(0, bash), ...EXPLORE_AGENT_CODE_SEARCH_TOOLS, ...base.slice(bash)]
    : base;
  return options.codemode ? [...tools, ...EXPLORE_AGENT_CODEMODE_TOOLS] : tools;
}

export function formatExploreAllowedToolsForAgentDescription(
  options: {
    embeddedSearchEnabled?: boolean;
    /** The session's own tools include FindFiles/SearchCode (children share them). */
    codeSearch?: boolean;
  } = {},
): string {
  const allowedTools = buildExploreAllowedTools(options);
  const allowedToolSet = new Set(allowedTools);
  const prioritizedTools = EXPLORE_AGENT_DESCRIPTION_TOOL_PRIORITY.filter((tool) =>
    allowedToolSet.has(tool),
  );
  const unprioritizedTools = allowedTools.filter(
    (tool) =>
      tool !== "ToolSearch" &&
      tool !== "DeferredToolCall" &&
      !EXPLORE_AGENT_DESCRIPTION_TOOL_PRIORITY_SET.has(tool),
  );
  return [...prioritizedTools, ...unprioritizedTools].join(", ");
}
