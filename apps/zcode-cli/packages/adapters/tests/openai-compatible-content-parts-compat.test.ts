import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import { streamText, type LanguageModel } from "ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelStreamEvent } from "@zcode/contracts";
import {
  createOpenAICompatibleContentPartsCompatFetch,
  hasNonEmptyContentParts,
  normalizeOpenAICompatibleContentParts,
} from "../src/model/openai-compatible-content-parts-compat.js";
import { TestAiSdkModelAdapter, TestProviderConfigFixture } from "./test-provider-config.js";
import { executeAdapterGenerateText, executeAdapterStreamText } from "./test-adapter-model.js";

type Frame = Record<string, unknown>;

const THINKING_HEAD = "The user says hi. ";
const THINKING_TAIL = "Answer briefly.";
const ANSWER_PARTS = ["Hello", " from", " GLM."];
const ANSWER = ANSWER_PARTS.join("");

function chunk(delta: Frame, finishReason: string | null = null): Frame {
  return {
    id: "chatcmpl-glm",
    object: "chat.completion.chunk",
    created: 1,
    model: "zai-glm-5-3-flash",
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

function thinkingPart(text: string): Frame {
  return { type: "thinking", thinking: [{ type: "text", text }] };
}

// zai-glm-5-3-flash 的流式形态：思考阶段、过渡帧（收尾 thinking + 首个 text）和回答阶段都是 chunk 列表。
function glmStreamFrames(): Frame[] {
  return [
    chunk({ role: "assistant", content: [thinkingPart(THINKING_HEAD)] }),
    chunk({
      content: [thinkingPart(THINKING_TAIL), { type: "text", text: ANSWER_PARTS[0] }],
    }),
    chunk({ content: [{ type: "text", text: ANSWER_PARTS[1] }] }),
    chunk({ content: [{ type: "text", text: ANSWER_PARTS[2] }] }),
    chunk({}, "stop"),
    {
      ...chunk({}),
      choices: [],
      usage: { prompt_tokens: 5, completion_tokens: 9, total_tokens: 14 },
    },
  ];
}

function glmJsonBody(): Frame {
  return {
    id: "chatcmpl-glm",
    object: "chat.completion",
    created: 1,
    model: "zai-glm-5-3-flash",
    choices: [
      {
        index: 0,
        finish_reason: "stop",
        message: {
          role: "assistant",
          content: [thinkingPart(THINKING_HEAD + THINKING_TAIL), { type: "text", text: ANSWER }],
        },
      },
    ],
    usage: { prompt_tokens: 5, completion_tokens: 9, total_tokens: 14 },
  };
}

function sseText(frames: Frame[]): string {
  return `${frames.map((frame) => `data: ${JSON.stringify(frame)}\n\n`).join("")}data: [DONE]\n\n`;
}

/** 按固定字节数切块下发，覆盖 SSE 帧跨网络 chunk 的拼接。 */
function sseResponse(text: string, sliceBytes = 7): Response {
  const bytes = new TextEncoder().encode(text);
  return new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        for (let offset = 0; offset < bytes.length; offset += sliceBytes) {
          controller.enqueue(bytes.slice(offset, offset + sliceBytes));
        }
        controller.close();
      },
    }),
    { headers: { "content-type": "text/event-stream", "x-request-id": "req-glm" } },
  );
}

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  const headers = new Headers(init.headers);
  if (!headers.has("content-type")) headers.set("content-type", "application/json");
  return new Response(JSON.stringify(body), { ...init, headers });
}

function rawCompatibleModel(
  fetch: typeof globalThis.fetch,
  baseURL = "https://mistral.example.test/v1",
): LanguageModel {
  return createOpenAICompatible({
    name: "mistral",
    baseURL,
    apiKey: "fixture-key",
    fetch,
  })("zai-glm-5-3-flash");
}

function installGlmProvider() {
  const requests: Array<{ stream: boolean }> = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: unknown, init?: RequestInit) => {
      const body = JSON.parse(typeof init?.body === "string" ? init.body : "{}") as Frame;
      const stream = body.stream === true;
      requests.push({ stream });
      return stream ? sseResponse(sseText(glmStreamFrames())) : jsonResponse(glmJsonBody());
    }),
  );
  const adapter = new TestAiSdkModelAdapter({
    env: { ZCODE_RUNTIME_ENV: "test" },
    registry: new TestProviderConfigFixture({
      env: {},
      providers: {
        mistral: {
          kind: "openai-compatible",
          baseURL: "https://mistral.example.test/v1",
          apiKey: "fixture-key",
        },
      },
    }),
    retry: { maxAttempts: 1 },
  });
  const request = {
    messages: [{ role: "user" as const, content: "hi" }],
    providerId: "mistral" as never,
    modelId: "zai-glm-5-3-flash" as never,
  };
  return { adapter, request, requests };
}

describe("OpenAI-compatible array content normalization", () => {
  it("splits a transition delta into reasoning_content and content", () => {
    const transition = chunk({
      content: [thinkingPart(THINKING_TAIL), { type: "text", text: ANSWER_PARTS[0] }],
    });

    expect(normalizeOpenAICompatibleContentParts(transition)).toEqual({
      ...transition,
      choices: [
        {
          index: 0,
          finish_reason: null,
          delta: { content: ANSWER_PARTS[0], reasoning_content: THINKING_TAIL },
        },
      ],
    });
  });

  it("keeps a thinking-only delta out of the visible content", () => {
    expect(
      normalizeOpenAICompatibleContentParts(
        chunk({ role: "assistant", content: [thinkingPart(THINKING_HEAD)] }),
      )?.choices,
    ).toEqual([
      {
        index: 0,
        finish_reason: null,
        delta: { role: "assistant", content: "", reasoning_content: THINKING_HEAD },
      },
    ]);
  });

  it("appends array thinking after existing string reasoning and drops unknown parts", () => {
    const normalized = normalizeOpenAICompatibleContentParts(
      chunk({
        reasoning_content: "a",
        content: [
          { type: "thinking", thinking: "b" },
          { type: "image_url", image_url: { url: "https://example.test/x.png" } },
          { type: "reference", text: "not visible" },
          { type: "text", text: "c" },
        ],
      }),
    );

    expect(normalized?.choices).toEqual([
      { index: 0, finish_reason: null, delta: { reasoning_content: "ab", content: "c" } },
    ]);
  });

  it("normalizes non-stream message content", () => {
    const normalized = normalizeOpenAICompatibleContentParts(glmJsonBody());

    expect(normalized?.choices).toEqual([
      {
        index: 0,
        finish_reason: "stop",
        message: {
          role: "assistant",
          content: ANSWER,
          reasoning_content: THINKING_HEAD + THINKING_TAIL,
        },
      },
    ]);
    expect(normalized?.usage).toEqual(glmJsonBody().usage);
  });

  it("leaves string, null, missing content and error bodies unchanged", () => {
    for (const body of [
      chunk({ content: "plain" }),
      chunk({ content: null, reasoning_content: "think" }),
      chunk({}),
      { ...chunk({}), choices: [] },
      { error: { message: "rate limited" } },
      "not an object",
    ]) {
      expect(normalizeOpenAICompatibleContentParts(body)).toBeUndefined();
    }
  });

  it("detects non-empty content parts for first-content timing", () => {
    expect(hasNonEmptyContentParts([{ type: "text", text: "a" }])).toBe(true);
    expect(hasNonEmptyContentParts([thinkingPart("t")])).toBe(true);
    expect(hasNonEmptyContentParts([{ type: "text", text: "" }])).toBe(false);
    expect(hasNonEmptyContentParts([])).toBe(false);
    expect(hasNonEmptyContentParts("a")).toBe(false);
  });
});

describe("OpenAI-compatible content parts compat fetch", () => {
  it("passes string-content streams through byte for byte", async () => {
    const original =
      `data: ${JSON.stringify(chunk({ role: "assistant", content: "" }))}\r\n\r\n` +
      `: keep-alive\n\n` +
      `data: ${JSON.stringify(chunk({ content: "Hel" }))}\n\n` +
      `data: ${JSON.stringify(chunk({ content: "lo" }, "stop"))}\n\n` +
      `data: [DONE]\n\n`;
    const compatFetch = createOpenAICompatibleContentPartsCompatFetch(async () =>
      sseResponse(original, 5),
    );

    const response = await compatFetch("https://mistral.example.test/v1/chat/completions");

    expect(response.headers.get("x-request-id")).toBe("req-glm");
    await expect(response.text()).resolves.toBe(original);
  });

  it("rewrites only the array-content frames of a stream", async () => {
    const stringFrame = `data: ${JSON.stringify(chunk({ content: "kept" }))}\n\n`;
    const arrayFrame = chunk({
      content: [thinkingPart(THINKING_TAIL), { type: "text", text: ANSWER_PARTS[0] }],
    });
    const compatFetch = createOpenAICompatibleContentPartsCompatFetch(async () =>
      sseResponse(`${stringFrame}data: ${JSON.stringify(arrayFrame)}\n\ndata: [DONE]\n\n`, 3),
    );

    const text = await (await compatFetch("https://mistral.example.test/v1")).text();

    expect(text).toBe(
      `${stringFrame}data: ${JSON.stringify(normalizeOpenAICompatibleContentParts(arrayFrame))}\n\ndata: [DONE]\n\n`,
    );
  });

  it("returns the original response for string JSON, errors and non-JSON bodies", async () => {
    const arrayBody = glmJsonBody();
    for (const response of [
      jsonResponse({ choices: [{ index: 0, message: { role: "assistant", content: "ok" } }] }),
      jsonResponse(arrayBody, { status: 400 }),
      new Response(JSON.stringify(arrayBody), { headers: { "content-type": "text/plain" } }),
      jsonResponse("not-an-object"),
    ]) {
      const compatFetch = createOpenAICompatibleContentPartsCompatFetch(async () => response);
      await expect(compatFetch("https://mistral.example.test/v1")).resolves.toBe(response);
    }
  });

  it("rewrites array JSON content and drops stale representation headers", async () => {
    const compatFetch = createOpenAICompatibleContentPartsCompatFetch(async () =>
      jsonResponse(glmJsonBody(), {
        headers: { "content-length": "999", "content-encoding": "gzip", "x-request-id": "r" },
      }),
    );

    const response = await compatFetch("https://mistral.example.test/v1");

    expect(response.headers.get("content-length")).toBeNull();
    expect(response.headers.get("content-encoding")).toBeNull();
    expect(response.headers.get("x-request-id")).toBe("r");
    await expect(response.json()).resolves.toMatchObject({
      choices: [{ message: { content: ANSWER, reasoning_content: THINKING_HEAD + THINKING_TAIL } }],
    });
  });

  it("fixes the SDK chunk schema failure that array content causes", async () => {
    const glmFetch = async () => sseResponse(sseText(glmStreamFrames()));

    // 根因证据：未经兼容层时 SDK chunk schema 拒绝数组 content（AI_TypeValidationError）。
    const errors: unknown[] = [];
    const raw = streamText({
      model: rawCompatibleModel(glmFetch),
      prompt: "hi",
      onError: ({ error }) => {
        errors.push(error);
      },
    });
    await raw.consumeStream();
    expect(errors.length).toBeGreaterThan(0);
    expect((errors[0] as Error).name).toBe("AI_TypeValidationError");

    const fixed = streamText({
      model: rawCompatibleModel(createOpenAICompatibleContentPartsCompatFetch(glmFetch)),
      prompt: "hi",
    });
    await expect(fixed.text).resolves.toBe(ANSWER);
    await expect(fixed.reasoningText).resolves.toBe(THINKING_HEAD + THINKING_TAIL);
  });
});

describe("Mistral reasoning history replay", () => {
  const MISTRAL_URL = "https://api.mistral.ai/v1/chat/completions";

  function historyBody(): Frame {
    return {
      model: "zai-glm-5-3-flash",
      messages: [
        { role: "user", content: "hi" },
        { role: "assistant", content: ANSWER, reasoning_content: THINKING_HEAD },
        { role: "user", content: "again" },
      ],
      stream: true,
    };
  }

  function captureFetch() {
    const bodies: Frame[] = [];
    const inits: Array<RequestInit | undefined> = [];
    const fetch = async (_input: unknown, init?: RequestInit) => {
      inits.push(init);
      bodies.push(JSON.parse(typeof init?.body === "string" ? init.body : "{}") as Frame);
      return sseResponse(sseText(glmStreamFrames()));
    };
    return { bodies, inits, fetch: fetch as typeof globalThis.fetch };
  }

  function assistantAt(bodies: Frame[]): Frame | undefined {
    const messages = bodies[0]?.messages;
    return Array.isArray(messages) ? (messages[1] as Frame) : undefined;
  }

  async function replayAfterReasoning(fetch: typeof globalThis.fetch): Promise<void> {
    const result = streamText({
      model: rawCompatibleModel(fetch, "https://api.mistral.ai/v1"),
      messages: [
        { role: "user", content: "hi" },
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: THINKING_HEAD },
            { type: "text", text: ANSWER },
          ],
        },
        { role: "user", content: "again" },
      ],
    });
    await result.consumeStream();
  }

  it("drops assistant reasoning_content the SDK replays to Mistral", async () => {
    // 根因证据：SDK 会把历史 reasoning 回放为 assistant.reasoning_content，Mistral 以 422 拒绝。
    const raw = captureFetch();
    await replayAfterReasoning(raw.fetch);
    expect(assistantAt(raw.bodies)).toMatchObject({ reasoning_content: THINKING_HEAD });

    const fixed = captureFetch();
    await replayAfterReasoning(createOpenAICompatibleContentPartsCompatFetch(fixed.fetch));
    const assistant = assistantAt(fixed.bodies);
    expect(assistant).toMatchObject({ role: "assistant", content: ANSWER });
    expect(assistant).not.toHaveProperty("reasoning_content");
  });

  it("keeps reasoning_content for non-Mistral hosts and untouched Mistral bodies", async () => {
    for (const [url, body] of [
      ["https://api.deepseek.com/v1/chat/completions", historyBody()],
      ["https://mistral.example.test/v1/chat/completions", historyBody()],
      [MISTRAL_URL, { model: "m", messages: [{ role: "user", content: "hi" }] }],
    ] as const) {
      const { inits, fetch } = captureFetch();
      const init = { method: "POST", body: JSON.stringify(body) };
      await createOpenAICompatibleContentPartsCompatFetch(fetch)(url, init);
      expect(inits[0]).toBe(init);
    }
  });

  it("strips only assistant messages and keeps the rest of the request", async () => {
    const { bodies, inits, fetch } = captureFetch();
    const body = historyBody();
    (body.messages as Frame[]).push({ role: "user", content: "x", reasoning_content: "kept" });
    const init = { method: "POST", headers: { "x-a": "1" }, body: JSON.stringify(body) };

    await createOpenAICompatibleContentPartsCompatFetch(fetch)(new URL(MISTRAL_URL), init);

    expect(inits[0]).toMatchObject({ method: "POST", headers: { "x-a": "1" } });
    expect(bodies[0]).toEqual({
      ...body,
      messages: [
        { role: "user", content: "hi" },
        { role: "assistant", content: ANSWER },
        { role: "user", content: "again" },
        { role: "user", content: "x", reasoning_content: "kept" },
      ],
    });
  });
});

describe("zai-glm-5-3-flash through the model adapter", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("renders a streamed reply whose delta.content is an array", async () => {
    const { adapter, request, requests } = installGlmProvider();

    const events: ModelStreamEvent[] = [];
    for await (const event of executeAdapterStreamText(adapter, request)) events.push(event);

    expect(requests).toEqual([{ stream: true }]);
    expect(events.filter((event) => event.type === "error")).toEqual([]);
    const text = events.flatMap((event) => (event.type === "text_delta" ? [event.text] : []));
    const reasoning = events.flatMap((event) =>
      event.type === "reasoning_delta" ? [event.text] : [],
    );
    expect(text.join("")).toBe(ANSWER);
    expect(reasoning.join("")).toBe(THINKING_HEAD + THINKING_TAIL);
    expect(text.join("")).not.toContain(THINKING_TAIL);
    const types = events.map((event) => event.type);
    expect(types.indexOf("reasoning_end")).toBeGreaterThan(-1);
    expect(types.indexOf("reasoning_end")).toBeLessThan(types.indexOf("text_start"));
    expect(events.at(-1)).toMatchObject({ type: "finish", finishReason: "stop" });
  });

  it("renders a non-stream reply whose message.content is an array", async () => {
    const { adapter, request, requests } = installGlmProvider();

    const result = await executeAdapterGenerateText(adapter, request);

    expect(requests).toEqual([{ stream: false }]);
    expect(result.text).toBe(ANSWER);
    expect(result.reasoning?.map((block) => block.text).join("")).toBe(
      THINKING_HEAD + THINKING_TAIL,
    );
  });
});
