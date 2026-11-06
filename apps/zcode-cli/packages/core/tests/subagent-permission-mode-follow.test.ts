import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createSessionId,
  createTraceId,
  createTurnId,
  type CollaborationMode,
} from "@zcode/contracts";
import { createRecordingSubagentSessionStore } from "./subagent-test-store.js";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestModelFactory } from "./test-runtime-model.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { AgentRuntime } from "../src/runtime.js";

// docs/subagent-permission-mode-follow.md：父会话运行中切换模式，存活的 child 必须跟随。
describe("Subagent 权限模式跟随父会话", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each([
    { name: "inherit", permissionMode: undefined, expected: "yolo" },
    { name: "explicit acceptEdits", permissionMode: "acceptEdits", expected: "edit" },
  ] as const)("$name：父 build → yolo 后 child 模式", async ({ permissionMode, expected }) => {
    const executeTurnSpy = vi.spyOn(AgentRuntime.prototype, "executeTurn");
    const childModes: { before?: CollaborationMode; after?: CollaborationMode } = {};
    let mainCalls = 0;
    let runtime: AgentRuntime | undefined;
    const usage = { inputTokens: 1, outputTokens: 1, totalTokens: 2 };
    runtime = createTestAgentRuntime(
      createSessionId(`mode-follow-${permissionMode ?? "inherit"}`),
      {
        mode: "build",
        workingDirectory: "/workspace/project",
        modelSelection: { providerId: "p", modelId: "m", options: { reasoningLevel: "high" } },
        subagents: {
          profiles: [
            {
              name: "worker",
              description: "Do work",
              source: "user",
              tools: ["Read"],
              systemPrompt: "Return a short result.",
              ...(permissionMode ? { permissionMode } : {}),
            },
          ],
        },
      },
      {
        eventStore: createTestSessionEventStore(),
        modelFactory: createTestModelFactory({
          async generateText(_request, observation) {
            const metadata = observation.invocationContext?.metadata as
              | { querySource?: string }
              | undefined;
            if (metadata?.querySource === "subagent") {
              const child = executeTurnSpy.mock.contexts.find((context) => context !== runtime);
              childModes.before = child?.getMode();
              // 子代理运行期间用户把父会话切到「完全访问」。
              await runtime!.setExecutionState({ mode: "yolo" });
              childModes.after = child?.getMode();
              return { finishReason: "stop", text: "child done", usage };
            }
            if (++mainCalls === 1)
              return {
                finishReason: "tool-calls",
                text: "",
                usage,
                toolCalls: [
                  {
                    id: "call_child",
                    name: "Agent",
                    input: { description: "Work", prompt: "Do work", subagent_type: "worker" },
                  },
                ],
              };
            return { finishReason: "stop", text: "done", usage };
          },
        }),
      },
    );
    await runtime.executeTurn("Run the worker.");
    expect(childModes.before).toBe(permissionMode ? "edit" : "build");
    expect(childModes.after).toBe(expected);
    expect(runtime.getMode()).toBe("yolo");
  });

  it("SendMessage 恢复已结束的 child 时使用父会话当前模式，而不是 child 落库的 build", async () => {
    const executeTurnSpy = vi.spyOn(AgentRuntime.prototype, "executeTurn");
    const sessionId = createSessionId("mode-follow-resume");
    const childModes: CollaborationMode[] = [];
    let runtime: AgentRuntime | undefined;
    runtime = createTestAgentRuntime(
      sessionId,
      { mode: "build", workingDirectory: "/workspace/project" },
      {
        eventStore: createTestSessionEventStore(),
        sessionStore: createRecordingSubagentSessionStore(),
        modelFactory: createTestModelFactory({
          async generateText() {
            const child = executeTurnSpy.mock.contexts.at(-1);
            if (child && child !== runtime) childModes.push(child.getMode());
            return {
              finishReason: "stop",
              text: "done",
              usage: { inputTokens: 1, outputTokens: 1, totalTokens: 2 },
            };
          },
        }),
      },
    );
    const port = (runtime as any).subagentPort;
    const turnId = createTurnId("mode-follow-resume-turn");
    const trace = { traceId: createTraceId("mode-follow-resume-trace"), sessionId, turnId };
    const base = {
      sessionId,
      trace,
      turnId,
      workingDirectory: "/workspace/project",
      workspaceRoot: "/workspace/project",
    };
    const first = await port.run({
      ...base,
      agentType: "general-purpose",
      description: "Work",
      parentToolCallId: "first",
      prompt: "Do work",
    });
    expect(first.status).toBe("completed");
    await runtime.setExecutionState({ mode: "yolo" });
    const resumed = await port.sendMessage({
      ...base,
      to: first.agentId,
      message: "Continue",
      summary: "Continue",
      parentToolCallId: "second",
    });
    expect(resumed.delivery).toBe("resumed_background");
    await port.waitForTask(first.agentId);
    expect(childModes).toEqual(["build", "yolo"]);
  });
});
