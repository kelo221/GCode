import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createProjectId, createSessionId } from "@zcode/contracts";
import type { ModelRequest, ModelResult, VoicePort } from "@zcode/contracts";
import { describe, expect, it, vi } from "vitest";
import { createSqliteSessionStore } from "../../adapters/src/storage/session-store/sqlite-session-store.js";
import { speakClosingStop } from "../src/runtime/methods/turn-tools.js";
import type { ToolExecutionResult } from "../src/tool/types.js";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { createTestModelFactory } from "./test-runtime-model.js";

function result(toolName: string): ToolExecutionResult {
  return {
    toolCallId: `call-${toolName}` as never,
    toolName,
    success: true,
    output: {},
    durationMs: 0,
    startedAt: new Date(0),
    completedAt: new Date(0),
  } as ToolExecutionResult;
}

describe("speakClosingStop", () => {
  it("ends the turn when reply text comes with only Speak calls", () => {
    const stop = speakClosingStop("All done.", [result("Speak")]);
    expect(stop?.turnControl).toEqual({ reason: "speak_closing", stopTurnAfterResult: true });
  });

  it("continues without reply text", () => {
    expect(speakClosingStop("  ", [result("Speak")])).toBeUndefined();
  });

  it("continues when another tool ran in the same step", () => {
    expect(speakClosingStop("Checking.", [result("Speak"), result("Bash")])).toBeUndefined();
  });

  it("continues with no results", () => {
    expect(speakClosingStop("Done.", [])).toBeUndefined();
  });
});

async function runTurn(steps: ModelResult[]) {
  const root = await mkdtemp(join(tmpdir(), "speak-close-"));
  const sessionId = createSessionId(`speak-close-${steps.length}`);
  const store = await createSqliteSessionStore({ dbPath: join(root, "s.sqlite") });
  await store.createSession({
    id: sessionId,
    projectID: createProjectId("speak-close"),
    slug: "speak",
    title: "Speak",
    directory: root,
    version: "test",
    time: { created: 1, updated: 1 },
  });
  const requests: ModelRequest[] = [];
  const speak = vi.fn<VoicePort["speak"]>(async () => ({ accepted: true }));
  const runtime = createTestAgentRuntime(
    sessionId,
    {
      mode: "yolo",
      modelStreaming: "off",
      titleGeneration: { enabled: false },
      workspacePath: root,
    },
    {
      sessionStore: store,
      eventStore: createTestSessionEventStore(),
      voicePort: { speak },
      modelFactory: createTestModelFactory({
        async generateText(request) {
          requests.push(request as ModelRequest);
          return steps[Math.min(requests.length, steps.length) - 1]!;
        },
      }),
    },
  );
  try {
    await runtime.executeTurn("Finish the task.");
    const parts = (await store.messages({ sessionID: sessionId })).flatMap((m) => m.parts);
    return { requests, speak, parts };
  } finally {
    await runtime.dispose?.();
    store.close?.();
    await rm(root, { recursive: true, force: true });
  }
}

const speakCall = {
  id: "call-speak",
  name: "Speak",
  input: { message: "The task is done.", kind: "done" },
};

describe("Speak closing a turn", () => {
  it("sends no follow-up request after reply text plus Speak", async () => {
    const { requests, speak, parts } = await runTurn([
      {
        text: "Here is the report.",
        finishReason: "tool-calls",
        usage: {},
        toolCalls: [speakCall],
      },
      { text: "Should not be requested.", finishReason: "stop", usage: {} },
    ]);
    expect(requests).toHaveLength(1);
    expect(speak).toHaveBeenCalledOnce();
    expect(
      parts.some((p) => p.type === "tool" && p.tool === "Speak" && p.state.status === "completed"),
    ).toBe(true);
  });

  it("still continues when Speak came without reply text", async () => {
    const { requests } = await runTurn([
      { text: "", finishReason: "tool-calls", usage: {}, toolCalls: [speakCall] },
      { text: "Here is the report.", finishReason: "stop", usage: {} },
    ]);
    expect(requests).toHaveLength(2);
  });

  it("keeps working after an info heads-up and rate-limits the next one", async () => {
    const heads = (id: string) => ({
      id,
      name: "Speak",
      input: { message: "Heads up, carrying on.", kind: "info" },
    });
    const read = (id: string) => ({ id, name: "Read", input: { file_path: "missing.txt" } });
    const { requests, speak } = await runTurn([
      {
        text: "Starting.",
        finishReason: "tool-calls",
        usage: {},
        toolCalls: [heads("call-h1"), read("call-r1")],
      },
      {
        text: "Still going.",
        finishReason: "tool-calls",
        usage: {},
        toolCalls: [heads("call-h2"), read("call-r2")],
      },
      { text: "Here is the report.", finishReason: "stop", usage: {} },
    ]);
    expect(requests).toHaveLength(3);
    expect(speak).toHaveBeenCalledOnce();
    const toolTexts = JSON.stringify(requests[2]?.messages);
    expect(toolTexts).toContain("heads-up only a few steps ago");
  });
});
