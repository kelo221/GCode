import { basename } from "node:path";
import { GENERAL_PURPOSE_AGENT_TYPE, buildGeneralPurposeSystemPrompt } from "./general-purpose.js";
import { EXPLORE_AGENT_TYPE } from "./explore.js";
import {
  formatExploreAllowedToolsForAgentDescription,
  withCodeSearchEquivalents,
} from "./explore-tools.js";
import { filterSubagentChildToolNames } from "./tool-policy.js";
import type { ModelSelection, AgentProfile } from "@zcode/shared";
export { parseAgentProfileFromMarkdown } from "@zcode/shared";
export type {
  AgentProfile,
  AgentPermissionMode,
  AgentProfileSource,
  AgentMemoryScope,
  AgentProfileParseDiagnostic,
  AgentProfileLoadResult,
} from "@zcode/shared";

export const DEFAULT_SUBAGENT_TYPE = GENERAL_PURPOSE_AGENT_TYPE;

export type BuiltInSubagentModelSelectionOverrides = Partial<
  Record<typeof DEFAULT_SUBAGENT_TYPE | typeof EXPLORE_AGENT_TYPE, ModelSelection>
>;

export function createBuiltInExploreAgentProfile(
  options: { modelSelection?: ModelSelection } = {},
): AgentProfile {
  return {
    name: EXPLORE_AGENT_TYPE,
    description:
      'Read-only search agent for broad fan-out searches — when answering means sweeping many files, directories, or naming conventions and you only need the conclusion, not the file dumps. It reads excerpts rather than whole files, so it locates code; it doesn\'t review or audit it. Specify search breadth: "medium" for moderate exploration, "very thorough" for multiple locations and naming conventions.',
    color: "cyan",
    injectAgentsMd: false,
    ...(options.modelSelection ? { modelSelection: options.modelSelection } : {}),
    source: "built-in",
    systemPrompt: "",
    tools: ["Bash", "Glob", "Grep", "Read", "WebFetch", "WebSearch", "TodoWrite"],
  };
}

export function isBuiltInExploreAgentProfile(
  profile: Pick<AgentProfile, "name" | "source">,
): boolean {
  // 用户/项目 profile 可以同名覆盖内置 Explore，不能只按名称套用内置行为。
  return profile.name === EXPLORE_AGENT_TYPE && profile.source === "built-in";
}

export function normalizeAgentProfiles(
  profiles: readonly AgentProfile[],
  options: {
    builtInModelSelectionOverrides?: BuiltInSubagentModelSelectionOverrides;
  } = {},
): AgentProfile[] {
  const overrides = options.builtInModelSelectionOverrides ?? {};
  const active = new Map<string, AgentProfile>();
  active.set(
    DEFAULT_SUBAGENT_TYPE,
    createBuiltInGeneralPurposeAgentProfile({
      modelSelection: overrides[DEFAULT_SUBAGENT_TYPE],
    }),
  );
  active.set(
    EXPLORE_AGENT_TYPE,
    createBuiltInExploreAgentProfile({
      modelSelection: overrides[EXPLORE_AGENT_TYPE],
    }),
  );
  for (const profile of profiles) {
    active.set(profile.name, profile);
  }
  return Array.from(active.values());
}

export function createBuiltInGeneralPurposeAgentProfile(
  options: { modelSelection?: ModelSelection } = {},
): AgentProfile {
  return {
    name: DEFAULT_SUBAGENT_TYPE,
    description:
      "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks. When you are searching for a keyword or file and are not confident that you will find the right match in the first few tries use this agent to perform the search for you.",
    // 内置子智能体使用显式身份色，避免 UI 按名称 hash 后把 general-purpose 显示为红色。
    color: "blue",
    injectAgentsMd: true,
    ...(options.modelSelection ? { modelSelection: options.modelSelection } : {}),
    source: "built-in",
    systemPrompt: buildGeneralPurposeSystemPrompt(),
    tools: ["*"],
  };
}

export function formatAgentProfilesForPrompt(
  profiles: readonly AgentProfile[],
  options: { embeddedSearchEnabled?: boolean } = {},
): string | null {
  const active = normalizeAgentProfiles(profiles);
  if (active.length === 0) return null;

  return [
    "Available agent types and the tools they have access to:",
    ...active.map((profile) => formatAgentProfileForPrompt(profile, options)),
  ].join("\n");
}

/** Glob/Grep are unregistered in embedded-search mode; FindFiles/SearchCode stand in. */
const EMBEDDED_SEARCH_REPLACED_TOOLS = new Set(["Glob", "Grep"]);

export function formatAgentProfileForPrompt(
  profile: Readonly<AgentProfile>,
  options: { embeddedSearchEnabled?: boolean; codeSearch?: boolean } = {},
): string {
  // 修复原因：子代理与父会话共享 FindFiles/SearchCode，但列表里不写，主模型据此误判
  // 「Explore 只能用 Bash rg」而放弃委派。父工具面确有这对工具时，列表照实写出运行时工具面。
  const profileTools =
    profile.tools && options.codeSearch
      ? withCodeSearchEquivalents(profile.tools).filter(
          (tool) => !(options.embeddedSearchEnabled && EMBEDDED_SEARCH_REPLACED_TOOLS.has(tool)),
        )
      : profile.tools;
  const tools = isBuiltInExploreAgentProfile(profile)
    ? formatExploreAllowedToolsForAgentDescription(options)
    : profileTools
      ? filterSubagentChildToolNames(profileTools, profile.disallowedTools)
      : undefined;
  const toolText = typeof tools === "string" ? tools : tools?.join(", ");
  return `- ${profile.name}: ${profile.description}${toolText ? ` (Tools: ${toolText})` : ""}`;
}

export function agentProfileDisplayName(profile: AgentProfile): string {
  return profile.path ? `${profile.name} (${basename(profile.path)})` : profile.name;
}
