// codemode store 的冷恢复 / rewind 重建（docs/design/v2/tool/21-codemode.md「store」）。
// 每个成功的 codemode tool part 在 metadata.codemodeStore 里存本次脚本的增量 `{set, delete}`；
// 按活动分支的消息顺序重放，就得到 live 时的当前值，分支与 rewind 自然跟随历史。

import { CODEMODE_TOOL_NAME, type MessageId, type MessageWithParts } from "@zcode/contracts";
import { activeSessionMessages } from "./session-history-hydrator.js";

interface StoreWrites {
  set: Record<string, unknown>;
  delete: string[];
}

function parseStoreWrites(metadata: unknown): StoreWrites | undefined {
  if (!metadata || typeof metadata !== "object") return undefined;
  const writes = (metadata as { codemodeStore?: unknown }).codemodeStore;
  if (!writes || typeof writes !== "object") return undefined;
  const { set, delete: deleted } = writes as { set?: unknown; delete?: unknown };
  if (!set || typeof set !== "object" || Array.isArray(set)) return undefined;
  if (!Array.isArray(deleted) || !deleted.every((key) => typeof key === "string")) return undefined;
  return { set: set as Record<string, unknown>, delete: deleted };
}

export function hydrateCodemodeStoreFromSession(input: {
  store: Map<string, unknown>;
  messages: MessageWithParts[];
  branchCutAfterMessageId?: MessageId;
  rewindCreatedMessageId?: MessageId;
  rewindKeptMessageIds?: readonly MessageId[];
  rewindTargetMessageId?: MessageId;
}): number {
  input.store.clear();
  const activeMessages = activeSessionMessages(input.messages, {
    branchCutAfterMessageId: input.branchCutAfterMessageId,
    includeCompactPreservedSegment: false,
    rewindCreatedMessageId: input.rewindCreatedMessageId,
    rewindKeptMessageIds: input.rewindKeptMessageIds,
    rewindTargetMessageId: input.rewindTargetMessageId,
  });
  let applied = 0;
  for (const message of activeMessages) {
    if (message.info.role !== "assistant") continue;
    const seen = new Set<string>();
    for (const part of message.parts) {
      if (seen.has(part.id)) continue;
      seen.add(part.id);
      if (part.type !== "tool" || part.tool !== CODEMODE_TOOL_NAME) continue;
      if (part.state.status !== "completed") continue;
      const writes = parseStoreWrites(part.state.metadata);
      if (!writes) continue;
      for (const key of writes.delete) input.store.delete(key);
      for (const [key, value] of Object.entries(writes.set)) input.store.set(key, value);
      applied += 1;
    }
  }
  return applied;
}
