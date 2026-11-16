import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  CoreErrorType,
  createCoreError,
  createSessionId,
  createToolCallId,
  createTraceId,
  createTurnId,
  isCoreError,
  type SessionId,
  type SubagentListEntry,
} from "@zcode/contracts";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { createTestModelFactory } from "./test-runtime-model.js";
import { createRecordingSubagentSessionStore } from "./subagent-test-store.js";
import {
  formatSubagentRosterReminder,
  readAgentMetadataSnapshot,
} from "../src/subagent/agent-index.js";
import { startRuntimeSubagentRosterRead } from "../src/runtime/helpers/agent-listing.js";
import { createErrorResult } from "../src/tool/executor/errors.js";
import { agentToolEntry } from "../src/tool/handlers/agent.js";
import { listAgentsToolEntry } from "../src/tool/handlers/list-agents.js";

const workspace = "/workspace/project";

async function setup(options: { fail?: (prompt: string) => boolean } = {}) {
  const outputRootDir = await mkdtemp(join(tmpdir(), "zcode-subagent-id-recovery-"));
  const sessionId = createSessionId(`id-recovery-${Math.random().toString(36).slice(2)}`);
  const requestMessages: string[] = [];
  const runtime = createTestAgentRuntime(
    sessionId,
    {
      mode: "yolo",
      workingDirectory: workspace,
      subagents: {
        outputRootDir,
        profiles: [
          {
            name: "reviewer",
            description: "Review",
            source: "user",
            systemPrompt: "Review",
            tools: ["Read"],
          },
        ],
      },
    },
    {
      eventStore: createTestSessionEventStore(),
      sessionStore: createRecordingSubagentSessionStore(),
      modelFactory: createTestModelFactory({
        async generateText(request) {
          const text = JSON.stringify(request.messages);
          requestMessages.push(text);
          if (options.fail?.(text)) throw new Error("Provider exploded");
          return {
            finishReason: "stop",
            text: "done",
            usage: { inputTokens: 1, outputTokens: 1, totalTokens: 2 },
          };
        },
      }),
    },
  );
  const internal = runtime as any;
  const turnId = createTurnId();
  const context = {
    sessionId,
    turnId,
    trace: { traceId: createTraceId(), sessionId, turnId },
    workingDirectory: workspace,
    workspaceRoot: workspace,
  };
  return { context, internal, outputRootDir, requestMessages, sessionId };
}

describe("subagent id recovery", () => {
  it("resumes an agent that is no longer in the registry from its metadata.json", async () => {
    const { context, internal, requestMessages } = await setup();
    const port = internal.subagentPort;
    const first = await port.run({
      ...context,
      agentType: "reviewer",
      description: "Review",
      parentToolCallId: "first",
      prompt: "ORIGINAL_CHILD_INPUT",
    });
    expect(first.status).toBe("completed");
    const original = await port.getTask(first.agentId);

    // 模拟 CLI 重启 / runtime 替换：内存 registry 丢了这个 agent。
    internal.runtimeTaskRegistry.remove(first.agentId);
    expect(await port.getTask(first.agentId)).toBeUndefined();

    const resumed = await port.sendMessage({
      ...context,
      to: first.agentId,
      message: "RESUME_CHILD_INPUT",
      summary: "Next",
      parentToolCallId: "second",
    });
    expect(resumed).toMatchObject({ status: "success", delivery: "resumed_background" });
    const task = await port.waitForTask(first.agentId);
    expect(task).toMatchObject({
      status: "completed",
      agentId: first.agentId,
      childSessionId: original.childSessionId,
    });
    expect(requestMessages.at(-1)).toContain("ORIGINAL_CHILD_INPUT");
    expect(requestMessages.at(-1)).toContain("RESUME_CHILD_INPUT");
  });

  it("points at ListAgents for unknown, malformed or foreign agent ids", async () => {
    const { context, internal, outputRootDir, sessionId } = await setup();
    const foreignId = "agent_foreign-1";
    await mkdir(join(outputRootDir, sessionId, foreignId), { recursive: true });
    await writeFile(
      join(outputRootDir, sessionId, foreignId, "metadata.json"),
      JSON.stringify({
        agentId: foreignId,
        childSessionId: "sess_subagent_agent_foreign-1",
        parentSessionId: "sess_someone_else",
        profileId: "reviewer",
        status: "completed",
      }),
    );
    for (const to of ["agent_missing", "agent_../../etc", foreignId]) {
      const result = await internal.subagentPort.sendMessage({
        ...context,
        to,
        message: "hi",
        summary: "hi",
        parentToolCallId: "call",
      });
      expect(result.status).toBe("failed");
      expect(result.message).toBe(
        `No subagent ${to} found in this session. Use ListAgents to see this session's subagents and their IDs.`,
      );
    }
    expect(await readAgentMetadataSnapshot(outputRootDir, sessionId, "agent_../x")).toBeUndefined();
  });

  it("treats an agent left running by a dead process as stopped", async () => {
    const outputRootDir = await mkdtemp(join(tmpdir(), "zcode-subagent-id-recovery-"));
    const sessionId = createSessionId("crashed") as SessionId;
    const agentId = "agent_crashed-1";
    await mkdir(join(outputRootDir, sessionId, agentId), { recursive: true });
    await writeFile(
      join(outputRootDir, sessionId, agentId, "metadata.json"),
      JSON.stringify({
        agentId,
        childSessionId: "sess_subagent_agent_crashed-1",
        createdAt: "2026-10-20T10:00:00.000Z",
        description: "Long review",
        parentSessionId: sessionId,
        profileId: "reviewer",
        status: "running",
        updatedAt: "2026-10-20T10:05:00.000Z",
      }),
    );
    expect(await readAgentMetadataSnapshot(outputRootDir, sessionId, agentId)).toMatchObject({
      agentId,
      agentType: "reviewer",
      status: "stopped",
      notified: true,
      type: "local_agent",
    });
  });

  it("lists registry and disk agents with live entries winning", async () => {
    const { context, internal } = await setup();
    const port = internal.subagentPort;
    const first = await port.run({
      ...context,
      agentType: "reviewer",
      description: "First review",
      parentToolCallId: "first",
      prompt: "one",
    });
    const second = await port.run({
      ...context,
      agentType: "reviewer",
      description: "Second review",
      parentToolCallId: "second",
      prompt: "two",
    });
    internal.runtimeTaskRegistry.remove(first.agentId);

    const listed: SubagentListEntry[] = await port.listAgents(context.sessionId);
    expect(listed.map((entry) => [entry.agentId, entry.live, entry.status])).toEqual(
      expect.arrayContaining([
        [first.agentId, false, "completed"],
        [second.agentId, true, "completed"],
      ]),
    );
    expect(listed).toHaveLength(2);
    expect(await port.listAgents(createSessionId("other"))).toEqual([]);
  });

  it("builds the post-compact roster from disk once read, and from the registry before that", async () => {
    const { context, internal } = await setup();
    const port = internal.subagentPort;
    const evicted = await port.run({
      ...context,
      agentType: "reviewer",
      description: "Evicted review",
      parentToolCallId: "first",
      prompt: "one",
    });
    const live = await port.run({
      ...context,
      agentType: "reviewer",
      description: "Live review",
      parentToolCallId: "second",
      prompt: "two",
    });
    internal.runtimeTaskRegistry.remove(evicted.agentId);

    const readBeforeSettle = startRuntimeSubagentRosterRead(internal);
    const early = readBeforeSettle()!.content as string;
    expect(early).toContain(live.agentId);
    expect(early).not.toContain(evicted.agentId);

    const read = startRuntimeSubagentRosterRead(internal);
    await new Promise((resolve) => setTimeout(resolve, 50));
    const settled = read()!.content as string;
    expect(settled).toContain(live.agentId);
    expect(settled).toContain(evicted.agentId);
    expect(settled).toContain("use SendMessage with to: '<agentId>'");
  });

  it("puts the agent id in the model-visible result when a foreground agent fails", async () => {
    const { context, internal } = await setup({ fail: (text) => text.includes("BREAK_ME") });
    let thrown: unknown;
    try {
      await internal.subagentPort.run({
        ...context,
        agentType: "reviewer",
        description: "Review",
        parentToolCallId: "first",
        prompt: "BREAK_ME",
      });
    } catch (error) {
      thrown = error;
    }
    expect(isCoreError(thrown)).toBe(true);
    const agentId = Object.keys(internal.runtimeTaskRegistry.all())[0]!;
    // 合并上游后 agentId 行由 Agent 工具的 failure handback 给出（executor 以 modelNote 传入，
    // 与 call-runner 相同），不再走 error.context.modelContentSuffix。
    const result = createErrorResult(
      { id: createToolCallId("agent-call"), name: "Agent", input: {} } as any,
      thrown as Error,
      undefined,
      { modelNote: agentToolEntry.formatErrorModelContent!(thrown) },
    );
    expect(result.error?.message).not.toContain(agentId);
    expect(result.modelContent).toContain(`agentId: ${agentId}`);
    expect(result.modelContent).toContain(`SendMessage with to: '${agentId}'`);
    // agentId 行只出现一次：handback 给出，runner 不再附重复后缀。
    expect(result.modelContent!.split(`agentId: ${agentId}`)).toHaveLength(2);
  });

  it("leaves error results without a suffix unchanged", () => {
    const result = createErrorResult(
      { id: createToolCallId("bash-call"), name: "Bash", input: {} } as any,
      createCoreError(CoreErrorType.ToolExecutionFailed, "boom"),
    );
    expect(result.modelContent).toBeUndefined();
  });

  it("keeps the agentId trailer when a completed result is over the size cap", () => {
    const content = agentToolEntry.formatModelContent!({
      status: "completed",
      agentId: "agent_big-1",
      agentType: "reviewer",
      description: "Big review",
      prompt: "p",
      content: [{ type: "text", text: "x".repeat(200_000) }],
      totalToolUseCount: 1,
      totalDurationMs: 5,
    }) as string;
    expect(Buffer.byteLength(content, "utf8")).toBeLessThanOrEqual(120_000);
    expect(content).toContain("agentId: agent_big-1");
    expect(content).toContain("[Subagent output truncated");
  });

  it("formats ListAgents output and the post-compact roster", async () => {
    const entries: SubagentListEntry[] = [
      {
        agentId: "agent_b",
        agentType: "Explore",
        description: "Find auth code",
        status: "stopped",
        startedAt: new Date("2026-10-20T10:00:00.000Z"),
        updatedAt: new Date("2026-10-20T10:10:00.000Z"),
        live: false,
      },
      {
        agentId: "agent_a",
        agentType: "reviewer",
        description: "Review",
        status: "completed",
        startedAt: new Date("2026-10-20T09:00:00.000Z"),
        updatedAt: new Date("2026-10-20T09:10:00.000Z"),
        live: true,
      },
    ];
    const output = await listAgentsToolEntry.handler({ statuses: ["stopped"] }, {
      sessionId: "sess_x",
      toolCallId: "call",
      subagentPort: { listAgents: async () => entries },
    } as any);
    const text = listAgentsToolEntry.formatModelContent!(output) as string;
    expect(text).toContain('<agents count="1" total="1">');
    expect(text).toContain('<agent id="agent_b" type="Explore" status="stopped" live="false"');
    expect(text).not.toContain("agent_a");

    const roster = formatSubagentRosterReminder(entries, 1)!;
    expect(roster).toContain("- agent_b · Explore · stopped");
    expect(roster).toContain("1 older subagents not shown");
    expect(formatSubagentRosterReminder([])).toBeUndefined();
  });
});
