import { describe, expect, it } from "vitest";
import {
  createSessionId,
  type ModelInputMessage,
  type ModelRequest,
  type SessionStorePort,
  type ToolPart,
} from "@zcode/contracts";
import { MessageHistoryImpl } from "../src/agent/message-history.js";
import { hydrateMessageHistoryFromSession } from "../src/agent/session-history-hydrator.js";
import { buildPostCompactRuntimeEntries } from "../src/runtime/helpers/compact.js";
import { buildProviderRequestMessages } from "../src/runtime/helpers/provider-request-messages.js";
import { createToolRegistry } from "../src/tool/registry.js";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { createRecordingMessageStore } from "./runtime-output-token-continuation-test-helpers.js";
import { createTestModelFactory } from "./test-runtime-model.js";

// 回归：Gemini thought signature 作为 tool-call providerOptions 到达 core 后，
// 必须在 live history、ToolPart 持久化、冷恢复与 compact 保留尾部中都留在原 tool call 上。
// 原实现在 history 与 ToolPart 落盘时只保留 id/name/input，下一轮请求缺签名被 400。
const SIGNED_OPTIONS = { google: { thoughtSignature: "c2lnbmF0dXJlLWZpeHR1cmU=" } };
const SIGNED_CALL = {
  id: "call_a",
  name: "SigRead",
  input: { file_path: "a.txt" },
  providerOptions: SIGNED_OPTIONS,
};
const UNSIGNED_CALL = { id: "call_b", name: "SigRead", input: { file_path: "b.txt" } };

// 上游给 tool call 加了响应内块序号 blockIndex（docs/design/v2/model/assistant-block-order.md），
// 它与签名无关；这里只比较身份、输入与 providerOptions。
function assistantToolCalls(messages: readonly ModelInputMessage[]) {
  return messages.flatMap((message) =>
    message.role === "assistant"
      ? (message.toolCalls ?? []).map(({ blockIndex: _blockIndex, ...call }) => call)
      : [],
  );
}

function createFixture() {
  const store = {
    ...createRecordingMessageStore(),
    async getProjectPermission() {
      return null;
    },
  };
  const requests: ModelRequest[] = [];
  const registry = createToolRegistry();
  registry.register({
    capability: "Read fixture for tool-call provider options",
    inputSchema: {},
    outputSchema: { type: "string" },
    timeout: { kind: "none" },
    cancellation: { supported: true, cleanup: "none", userVisibleMessage: "Read cancelled" },
    permission: {
      permission: "read",
      reason: "Read fixture",
      riskLevel: "low",
      sideEffectScope: "none",
      needsApproval: false,
      patternSources: ["none"],
      denyPriority: "beforeAsk",
    },
    resultBudget: { maxInlineBytes: 1024, maxModelBytes: 1024, strategy: "inline" },
    trace: {
      required: true,
      propagateToAdapters: true,
      recordInput: "summary",
      recordOutput: "summary",
    },
    metadata: {
      name: "SigRead",
      concurrentSafe: true,
      destructive: false,
      needsApproval: false,
      readOnly: true,
      riskLevel: "low",
      sideEffectScope: "none",
    },
    handler: async () => "hello",
  });
  const runtime = createTestAgentRuntime(
    createSessionId("tool-call-provider-options"),
    {
      mode: "yolo",
      toolSearch: { mode: "off" },
      modelStreaming: "on",
      titleGeneration: { enabled: false },
    },
    {
      eventStore: createTestSessionEventStore(),
      sessionStore: store as unknown as SessionStorePort,
      toolRegistry: registry,
      modelFactory: createTestModelFactory({
        async *streamText(request) {
          requests.push(request);
          if (requests.length === 1) {
            yield { type: "tool_call", toolCall: SIGNED_CALL };
            yield { type: "tool_call", toolCall: UNSIGNED_CALL };
            yield { type: "finish", finishReason: "tool-calls", usage: {} };
            return;
          }
          yield { type: "text_delta", text: "done" };
          yield { type: "finish", finishReason: "stop", usage: {} };
        },
      }),
    },
  );
  return { runtime, store, requests };
}

describe("tool-call providerOptions (Gemini thought signature) history", () => {
  it("keeps the signature on the same call in the next live request, ToolParts and cold restore", async () => {
    const fixture = createFixture();
    await fixture.runtime.executeTurn("read both");

    expect(fixture.requests).toHaveLength(2);
    expect(assistantToolCalls(fixture.requests[1]!.messages)).toEqual([SIGNED_CALL, UNSIGNED_CALL]);

    const finalParts = new Map<string, ToolPart>();
    for (const part of fixture.store.savedParts) {
      if (part.type === "tool") finalParts.set(part.callID, part);
    }
    expect(finalParts.get("call_a")?.metadata).toMatchObject({ providerOptions: SIGNED_OPTIONS });
    expect(finalParts.get("call_b")?.metadata?.providerOptions).toBeUndefined();

    const history = new MessageHistoryImpl();
    await hydrateMessageHistoryFromSession({ history, messages: await fixture.store.messages() });
    const hydrated = buildProviderRequestMessages({ entries: history.toRuntimeEntries() });
    expect(assistantToolCalls(hydrated.messages)).toEqual([SIGNED_CALL, UNSIGNED_CALL]);
  });

  it("survives history copies, compact preserved entries and wrapper projection", () => {
    const history = new MessageHistoryImpl();
    history.addAssistant("", [SIGNED_CALL, UNSIGNED_CALL]);
    history.addToolResult("call_a", "SigRead", "hello", true);
    history.addToolResult("call_b", "SigRead", "hello", true);
    history.replaceMessages(history.toRuntimeEntries());
    const entries = history.toRuntimeEntries();

    const compacted = buildPostCompactRuntimeEntries(
      [],
      { message: { role: "user", content: "summary" } },
      { preservedEntries: entries },
    );
    const projected = buildProviderRequestMessages({ entries: compacted });
    expect(assistantToolCalls(projected.messages)).toEqual([SIGNED_CALL, UNSIGNED_CALL]);

    const wrapped = new MessageHistoryImpl();
    wrapped.addAssistant("", [{ ...SIGNED_CALL, modelCallWrapper: "DeferredToolCall" }]);
    expect(
      assistantToolCalls(
        buildProviderRequestMessages({ entries: wrapped.toRuntimeEntries() }).messages,
      ),
    ).toEqual([
      {
        id: "call_a",
        name: "DeferredToolCall",
        input: { name: "SigRead", input: { file_path: "a.txt" } },
        providerOptions: SIGNED_OPTIONS,
      },
    ]);
  });
});
