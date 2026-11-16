import { buildToolExposure } from "../../tool/search/exposure.js";
import { applyCodemodeOnlyExposure } from "../../tool/codemode/catalog.js";
import type { ModelToolContract } from "@zcode/contracts";
import type { RuntimeMessageEntry } from "../../agent/message-history.js";
import type { ToolRegistry } from "../../tool/registry.js";
import type { Model } from "../deps.js";
import type { AgentRuntimeConfig } from "../types.js";
import {
  buildProviderRequestMessages,
  type ProviderRequestMessageProjectionResult,
} from "./provider-request-messages.js";

/** 本次请求是否走 MCS 的唯一判断：provider 投影与 system prompt 的 Harness 文案共用。 */
export function shouldUseMidConversationSystem(
  config: Pick<AgentRuntimeConfig, "midConversationSystem">,
  model: Pick<Model, "properties">,
): boolean {
  return (
    config.midConversationSystem?.mode === "force" || model.properties.supportsMidConversationSystem
  );
}

export function shouldFoldRemindersIntoToolResults(
  model: Pick<Model, "requestCapabilities">,
): boolean {
  return model.requestCapabilities?.foldRemindersIntoToolResults === true;
}

type RequestRuntime = {
  readonly config: Pick<AgentRuntimeConfig, "midConversationSystem" | "toolSearch" | "codemode">;
};

interface MessageProjectionInput {
  entries: readonly RuntimeMessageEntry[];
  applyCacheControl?: boolean;
  skipCacheWrite?: boolean;
  model: Model;
  loadedToolCallIds?: ReadonlySet<string>;
}

// Bug 根因：辅助请求分别组装历史和 tools 时，曾按模型能力保留引用却发送 tools: []。
// 请求准备必须先用完整作用域目录做一次 ToolSearch 决策，再向投影和发送方提供配套结果。
export function prepareRuntimeProviderRequest(
  runtime: RequestRuntime,
  input: MessageProjectionInput & {
    tools: readonly ModelToolContract[];
    registry?: Pick<ToolRegistry, "get">;
    prompt?: string;
  },
) {
  const codemodeOnly = runtime.config.codemode?.mode === "only";
  const exposure = buildToolExposure({
    model: input.model,
    // 修复原因：`only` 模式会去掉 ToolSearch，但旧代码仍按 toolSearch 配置延迟 MCP 工具——native 时
    // 请求带 deferLoading 却没有加载入口，local 时被延迟的工具在 codemode 说明里完全不出现。
    // `only` 由 codemode 自己的说明预算负责"延迟"，工具搜索在这里整体关闭。
    mode: codemodeOnly ? "off" : runtime.config.toolSearch?.mode,
    tools: input.tools,
    registry: input.registry,
    entries: input.entries,
  });
  const projection = buildRuntimeProviderRequestMessages(runtime, {
    entries:
      input.prompt === undefined
        ? exposure.entries
        : [...exposure.entries, { message: { role: "user", content: input.prompt } }],
    model: input.model,
    loadedToolCallIds: exposure.loadedToolCallIds,
    applyCacheControl: input.applyCacheControl,
    skipCacheWrite: input.skipCacheWrite,
  });
  return {
    request: {
      messages: projection.messages,
      // `only`：其他工具只经 codemode 脚本调用（docs/design/v2/tool/21-codemode.md）。
      tools: codemodeOnly ? applyCodemodeOnlyExposure(exposure.tools) : exposure.tools,
      toolSearchEnabled: exposure.context.mode === "native",
    },
    projection,
    entries: exposure.entries,
    // 记录过滤 Continue 后会重新渲染，需复用本次有效引用，避免遗漏成功提示或重复扫描历史。
    loadedToolCallIds: exposure.loadedToolCallIds,
    toolSearch: exposure.context,
    reason: exposure.reason,
  };
}

// 仅渲染消息，供已准备请求的记录投影和历史估算复用；不重新猜测工具启用状态。
export function buildRuntimeProviderRequestMessages(
  runtime: RequestRuntime,
  input: MessageProjectionInput,
): ProviderRequestMessageProjectionResult {
  return buildProviderRequestMessages({
    ...input,
    useMidConversationSystem: shouldUseMidConversationSystem(runtime.config, input.model),
    foldRemindersIntoToolResults: shouldFoldRemindersIntoToolResults(input.model),
  });
}
