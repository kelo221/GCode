import type { ModelInputMessage } from "@zcode/contracts";

/**
 * Anthropic prompt cache 生命周期（GPUI 每个 Provider 卡片的「Prompt cache」，apps/zcode-gpui/PARITY.md）。
 * `5m` 为默认：不改动 runtime 已放置的 marker，wire 与旧行为逐字节一致。
 */
export type PromptCacheMode = "5m" | "1h" | "off";

/** 按 providerId 取值；同一会话可混用真 Anthropic 与拒绝 ttl 的代理，所以不能全局一个值。 */
export type PromptCacheModes = Readonly<Record<string, PromptCacheMode>>;

export const DEFAULT_PROMPT_CACHE_MODE: PromptCacheMode = "5m";

export function promptCacheModeFor(modes: PromptCacheModes, providerId: string): PromptCacheMode {
  return Object.hasOwn(modes, providerId) ? modes[providerId]! : DEFAULT_PROMPT_CACHE_MODE;
}

/**
 * 统一改写一次逻辑请求内的全部 cache marker：Anthropic 要求更长 TTL 的断点排在更短的之前，
 * 逐条混用会被拒，所以只能整请求同一取值。没有变化时返回原数组。
 */
export function applyPromptCacheMode(
  messages: ModelInputMessage[],
  mode: PromptCacheMode,
): ModelInputMessage[] {
  if (mode === "5m") return messages;
  let changed = false;
  const next = messages.map((message) => {
    if (!message.cacheControl) return message;
    changed = true;
    if (mode === "off") {
      const { cacheControl: _removed, ...rest } = message;
      return rest;
    }
    return { ...message, cacheControl: { ...message.cacheControl, ttl: "1h" as const } };
  });
  return changed ? next : messages;
}
