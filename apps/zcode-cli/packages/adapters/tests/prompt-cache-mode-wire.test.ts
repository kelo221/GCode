import { createServer, type Server } from "node:http";

import { createModelId, type ModelInputMessage } from "@zcode/contracts";
import { afterEach, describe, expect, it } from "vitest";

import {
  applyPromptCacheMode,
  promptCacheModeFor,
  type PromptCacheModes,
} from "../src/model/prompt-cache-mode.js";
import {
  TestAiSdkModelAdapter as AiSdkModelAdapter,
  TestProviderConfigFixture,
} from "./test-provider-config.js";
import { executeAdapterGenerateText, executeAdapterStreamText } from "./test-adapter-model.js";

const MODEL_ID = createModelId("claude-opus-4-8-cc");
const MARKER = { type: "ephemeral" as const };
const MESSAGES: ModelInputMessage[] = [
  { role: "system", content: "stable system prompt", cacheControl: MARKER },
  { role: "user", content: "earlier question" },
  { role: "assistant", content: "earlier answer" },
  { role: "user", content: "latest question", cacheControl: MARKER },
];

const openServers: Server[] = [];

afterEach(async () => {
  await Promise.all(openServers.splice(0).map(closeServer));
});

describe("prompt cache mode on the Anthropic wire", () => {
  it.each([
    { mode: undefined, expected: { type: "ephemeral" } },
    { mode: "5m" as const, expected: { type: "ephemeral" } },
    { mode: "1h" as const, expected: { type: "ephemeral", ttl: "1h" } },
    { mode: "off" as const, expected: undefined },
  ])("mode $mode marks system and latest user with $expected", async ({ mode, expected }) => {
    const capture = await startCapture();
    const adapter = createAdapter(capture.baseURL, mode ? { anthropic: mode } : undefined);

    await sendStreaming(adapter, MESSAGES);
    await sendGenerate(adapter, MESSAGES);

    expect(capture.bodies).toHaveLength(2);
    for (const body of capture.bodies) {
      expect(body.system).toEqual([
        { type: "text", text: "stable system prompt", ...cacheField(expected) },
      ]);
      const messages = body.messages as Array<{ content: Array<Record<string, unknown>> }>;
      expect(messages.at(-1)?.content.at(-1)).toEqual({
        type: "text",
        text: "latest question",
        ...cacheField(expected),
      });
      expect(JSON.stringify(messages.slice(0, -1))).not.toContain("cache_control");
    }
  });

  it("follows setPromptCacheModes on the next request", async () => {
    const capture = await startCapture();
    const adapter = createAdapter(capture.baseURL, undefined);

    adapter.setPromptCacheModes({ anthropic: "1h" });
    await sendGenerate(adapter, MESSAGES);
    adapter.setPromptCacheModes({});
    await sendGenerate(adapter, MESSAGES);

    expect(capture.bodies.map((body) => body.system)).toEqual([
      [
        {
          type: "text",
          text: "stable system prompt",
          cache_control: { type: "ephemeral", ttl: "1h" },
        },
      ],
      [{ type: "text", text: "stable system prompt", cache_control: { type: "ephemeral" } }],
    ]);
  });

  it("applies each provider's own entry and never another's", async () => {
    const capture = await startCapture();
    const adapter = createAdapter(capture.baseURL, { proxy: "off" });

    await sendGenerate(adapter, MESSAGES, "anthropic");
    await sendGenerate(adapter, MESSAGES, "proxy");

    expect(capture.bodies[0]?.system).toEqual([
      { type: "text", text: "stable system prompt", cache_control: { type: "ephemeral" } },
    ]);
    expect(JSON.stringify(capture.bodies[1])).not.toContain("cache_control");
  });
});

describe("applyPromptCacheMode", () => {
  it("defaults missing and inherited keys to 5m", () => {
    expect(promptCacheModeFor({ a: "1h" }, "a")).toBe("1h");
    expect(promptCacheModeFor({ a: "1h" }, "b")).toBe("5m");
    expect(promptCacheModeFor({}, "toString")).toBe("5m");
  });

  it("returns the same array when nothing changes and never mutates the input", () => {
    const unmarked: ModelInputMessage[] = [{ role: "user", content: "hi" }];
    expect(applyPromptCacheMode(MESSAGES, "5m")).toBe(MESSAGES);
    expect(applyPromptCacheMode(unmarked, "1h")).toBe(unmarked);
    expect(applyPromptCacheMode(unmarked, "off")).toBe(unmarked);

    const longLived = applyPromptCacheMode(MESSAGES, "1h");
    expect(longLived[0]?.cacheControl).toEqual({ type: "ephemeral", ttl: "1h" });
    expect(MESSAGES[0]?.cacheControl).toEqual({ type: "ephemeral" });
    expect(applyPromptCacheMode(MESSAGES, "off").some((m) => "cacheControl" in m)).toBe(false);
  });
});

function cacheField(expected: Record<string, string> | undefined): Record<string, unknown> {
  return expected ? { cache_control: expected } : {};
}

function createAdapter(baseURL: string, modes: PromptCacheModes | undefined): AiSdkModelAdapter {
  return new AiSdkModelAdapter({
    registry: new TestProviderConfigFixture({
      env: {},
      providers: {
        anthropic: { apiKey: "prompt-cache-test-key", baseURL, kind: "anthropic" },
        proxy: { apiKey: "prompt-cache-test-key", baseURL, kind: "anthropic" },
      },
    }),
    retry: { maxAttempts: 1 },
    ...(modes ? { promptCacheModes: modes } : {}),
  });
}

async function sendGenerate(
  adapter: AiSdkModelAdapter,
  messages: ModelInputMessage[],
  providerId = "anthropic",
) {
  try {
    await executeAdapterGenerateText(adapter, {
      providerId: providerId as never,
      modelId: MODEL_ID,
      messages,
    });
  } catch {
    // capture endpoint 记录 body 后固定返回 400。
  }
}

async function sendStreaming(adapter: AiSdkModelAdapter, messages: ModelInputMessage[]) {
  try {
    for await (const _event of executeAdapterStreamText(adapter, {
      providerId: "anthropic" as never,
      modelId: MODEL_ID,
      messages,
    })) {
      // capture endpoint 不产生流事件。
    }
  } catch {
    // capture endpoint 记录 body 后固定返回 400。
  }
}

async function startCapture(): Promise<{ baseURL: string; bodies: Record<string, unknown>[] }> {
  const bodies: Record<string, unknown>[] = [];
  const server = createServer(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk.toString();
    bodies.push(JSON.parse(body) as Record<string, unknown>);
    response.writeHead(400, { "content-type": "application/json" });
    response.end(
      JSON.stringify({
        error: { message: "captured", type: "invalid_request_error" },
        type: "error",
      }),
    );
  });
  openServers.push(server);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("capture server has no port");
  return { baseURL: `http://127.0.0.1:${address.port}`, bodies };
}

async function closeServer(server: Server): Promise<void> {
  if (!server.listening) return;
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}
