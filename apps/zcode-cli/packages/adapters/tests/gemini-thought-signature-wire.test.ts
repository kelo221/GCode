import { afterEach, describe, expect, it, vi } from "vitest";
import {
  runWithModelInvocationContext,
  type ModelInputMessage,
  type ModelStreamEvent,
  type ModelToolCall,
} from "@zcode/contracts";
import { TestAiSdkModelAdapter, TestProviderConfigFixture } from "./test-provider-config.js";

// 回归：Gemini（Google OpenAI-compat）在并行调用的第一个 tool call 上返回
// extra_content.google.thought_signature；下一次请求必须在同一个 tool call 上带回，
// 否则 400 "Function call is missing a thought_signature"。见 model README §3.1.4。
const SIGNATURE = "c2lnbmF0dXJlLWZpeHR1cmU=";
const READ_TOOL = {
  name: "Read",
  inputSchema: {
    type: "object",
    properties: { file_path: { type: "string" } },
    required: ["file_path"],
  },
};

// 与真实端点一致：Google 不发送 tool_calls[].index，签名只在第一个调用上。
const GEMINI_PARALLEL_TOOL_FRAMES = [
  {
    choices: [
      {
        index: 0,
        delta: {
          role: "assistant",
          tool_calls: [
            {
              extra_content: { google: { thought_signature: SIGNATURE } },
              function: { arguments: '{"file_path":"a.txt"}', name: "Read" },
              id: "call_a",
              type: "function",
            },
          ],
        },
      },
    ],
  },
  {
    choices: [
      {
        index: 0,
        delta: {
          role: "assistant",
          tool_calls: [
            {
              function: { arguments: '{"file_path":"b.txt"}', name: "Read" },
              id: "call_b",
              type: "function",
            },
          ],
        },
      },
    ],
  },
  { choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] },
];

const TEXT_FRAMES = [
  { choices: [{ index: 0, delta: { role: "assistant", content: "done" } }] },
  { choices: [{ index: 0, delta: {}, finish_reason: "stop" }] },
];

function sse(frames: readonly Record<string, unknown>[]): Response {
  return new Response(
    [...frames.map((frame) => `data: ${JSON.stringify(frame)}`), "data: [DONE]", ""].join("\n\n"),
    { headers: { "content-type": "text/event-stream" } },
  );
}

function harness(responses: Response[]) {
  const bodies: Array<Record<string, unknown>> = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      bodies.push(JSON.parse(String(init?.body)) as Record<string, unknown>);
      const next = responses.shift();
      if (!next) throw new Error("unexpected extra request");
      return next;
    }),
  );
  const adapter = new TestAiSdkModelAdapter({
    env: { ZCODE_RUNTIME_ENV: "test" },
    registry: new TestProviderConfigFixture({
      env: {},
      providers: {
        google: {
          kind: "openai-compatible",
          baseURL: "https://generativelanguage.example.test/v1beta/openai",
          apiKey: "fixture-key",
        },
      },
    }),
    retry: { maxAttempts: 1, baseDelayMs: 0, maxDelayMs: 0, jitter: false },
  });
  const model = adapter.createModel({ providerId: "google", modelId: "gemini-test" });
  return {
    bodies,
    async stream(messages: ModelInputMessage[]): Promise<ModelStreamEvent[]> {
      const events: ModelStreamEvent[] = [];
      await runWithModelInvocationContext({ metadata: { querySource: "main_turn" } }, async () => {
        for await (const event of model.streamText({ messages, tools: [READ_TOOL] })) {
          events.push(event);
        }
      });
      return events;
    },
  };
}

function toolCallsOf(events: ModelStreamEvent[]): ModelToolCall[] {
  return events.flatMap((event) => (event.type === "tool_call" ? [event.toolCall] : []));
}

function secondTurnMessages(
  toolCalls: ModelToolCall[],
  owner: { providerId: string; modelId: string },
): ModelInputMessage[] {
  return [
    { role: "user", content: "read both" },
    {
      role: "assistant",
      content: "",
      toolCalls,
      providerId: owner.providerId as ModelInputMessage["providerId"],
      modelId: owner.modelId as ModelInputMessage["modelId"],
    },
    ...toolCalls.map(
      (toolCall): ModelInputMessage => ({
        role: "tool",
        content: "hello",
        toolCallId: toolCall.id,
        toolName: toolCall.name,
      }),
    ),
  ];
}

function assistantToolCalls(body: Record<string, unknown> | undefined) {
  const messages = (body?.messages ?? []) as Array<Record<string, unknown>>;
  return messages.find((message) => message.role === "assistant")?.tool_calls as
    | Array<Record<string, unknown>>
    | undefined;
}

describe("Gemini thought signature replay over OpenAI-compatible Chat Completions", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("captures the signature from the stream and echoes it on the same tool call next request", async () => {
    const h = harness([sse(GEMINI_PARALLEL_TOOL_FRAMES), sse(TEXT_FRAMES)]);

    const toolCalls = toolCallsOf(await h.stream([{ role: "user", content: "read both" }]));
    expect(toolCalls.map((toolCall) => toolCall.id)).toEqual(["call_a", "call_b"]);
    expect(JSON.stringify(toolCalls[0]?.providerOptions)).toContain(SIGNATURE);
    expect(toolCalls[1]?.providerOptions).toBeUndefined();

    await h.stream(secondTurnMessages(toolCalls, { providerId: "google", modelId: "gemini-test" }));

    const replayed = assistantToolCalls(h.bodies[1]);
    expect(replayed?.map((toolCall) => toolCall.id)).toEqual(["call_a", "call_b"]);
    expect(replayed?.[0]?.extra_content).toEqual({
      google: { thought_signature: SIGNATURE },
    });
    // 并行调用中没有签名的调用保持原样，不伪造签名。
    expect(replayed?.[1]).not.toHaveProperty("extra_content");
  });

  it("does not replay a signature produced by a different model", async () => {
    const h = harness([sse(GEMINI_PARALLEL_TOOL_FRAMES), sse(TEXT_FRAMES)]);
    const toolCalls = toolCallsOf(await h.stream([{ role: "user", content: "read both" }]));

    await h.stream(secondTurnMessages(toolCalls, { providerId: "google", modelId: "other-model" }));

    const replayed = assistantToolCalls(h.bodies[1]);
    expect(replayed?.map((toolCall) => toolCall.id)).toEqual(["call_a", "call_b"]);
    expect(replayed?.some((toolCall) => "extra_content" in toolCall)).toBe(false);
  });
});
