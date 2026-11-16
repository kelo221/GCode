import { describe, expect, it } from "vitest";
import { createSessionId } from "@zcode/contracts";
import { withoutAgentListingMessages } from "./test-agent-listing.js";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { createTestModelFactory } from "./test-runtime-model.js";

type Request = Array<{ role: string; content: string }>;

// A sentence of the default body (the environment moved to the environment announcement upstream).
const DEFAULT_BODY = "You are an agent working with the user";

function text(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((block) => (block && typeof block === "object" && "text" in block ? block.text : ""))
      .join("\n");
  }
  return String(content ?? "");
}

function systemText(request: Request | undefined): string {
  return (request ?? [])
    .filter((message) => message.role === "system")
    .map((message) => message.content)
    .join("\n");
}

function createRuntime(
  onRequest?: (runtime: { setSystemPromptOverride(p?: string): boolean }) => void,
) {
  const requests: Request[] = [];
  const runtime = createTestAgentRuntime(
    createSessionId(`system-prompt-override-${Math.random()}`),
    {
      currentDate: "2026-10-19",
      envInfo: {
        cwd: "/workspace",
        platform: "linux",
        shell: "bash",
        osVersion: "Linux 6.6.0",
        nodeVersion: "v24.14.0",
        isGitRepository: false,
      },
    },
    {
      eventStore: createTestSessionEventStore(),
      modelFactory: createTestModelFactory({
        async generateText(request) {
          requests.push(
            withoutAgentListingMessages(request.messages).map((message) => ({
              content: text(message.content),
              role: message.role,
            })),
          );
          onRequest?.(runtime);
          return {
            finishReason: "stop",
            providerMetadata: undefined,
            text: "ok",
            usage: { inputTokens: 1, outputTokens: 1, totalTokens: 2 },
          };
        },
      } as never),
    },
  );
  return { runtime, requests };
}

describe("system prompt override", () => {
  it("replaces the default body on the next turn and reset restores it", async () => {
    const { runtime, requests } = createRuntime();

    await runtime.executeTurn("first");
    expect(systemText(requests[0])).toContain(DEFAULT_BODY);

    expect(runtime.setSystemPromptOverride("Talk like a pirate.")).toBe(true);
    await runtime.executeTurn("second");
    const custom = systemText(requests[1]);
    expect(custom).toContain("You are GCode, an interactive coding agent");
    expect(custom).toContain("Talk like a pirate.");
    expect(custom).not.toContain(DEFAULT_BODY);

    expect(runtime.setSystemPromptOverride("   ")).toBe(true);
    await runtime.executeTurn("third");
    expect(systemText(requests[2])).toContain(DEFAULT_BODY);
    expect(systemText(requests[2])).not.toContain("Talk like a pirate.");
  });

  it("defers a change made during a turn to the following turn", async () => {
    let applied = false;
    const { runtime, requests } = createRuntime((live) => {
      if (applied) return;
      applied = true;
      live.setSystemPromptOverride("Answer in haiku.");
    });

    await runtime.executeTurn("first");
    expect(systemText(requests[0])).not.toContain("Answer in haiku.");

    await runtime.executeTurn("second");
    expect(systemText(requests[1])).toContain("Answer in haiku.");
  });

  it("previews the default body without the CLI prefix while an override is active", async () => {
    const { runtime } = createRuntime();
    await runtime.executeTurn("first");
    runtime.setSystemPromptOverride("Custom.");

    const preview = runtime.previewDefaultSystemPrompt();
    expect(preview).toContain(DEFAULT_BODY);
    expect(preview).not.toContain("Custom.");
    expect(preview.startsWith("You are GCode, an interactive coding agent")).toBe(false);
  });
});
