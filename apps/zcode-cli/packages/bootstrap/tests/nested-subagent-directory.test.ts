import { describe, expect, it } from "vitest";
import { SessionEventType, type SessionEvent } from "@zcode/contracts";
import { listSessionSubagents } from "../src/zcode-protocol/server-operations.js";
import type { ZCodeProtocolAgentServerContext } from "../src/zcode-protocol/server-types.js";

const MAIN = "sess_main";
const CHILD = "sess_subagent_agent_child";
const GRANDCHILD = "sess_subagent_agent_grand";

const session = (id: string, parentID?: string) => ({
  id,
  ...(parentID ? { parentID, taskType: "subagent_child" } : {}),
  time: { created: 1, updated: 2 },
});

const event = (sessionId: string, type: string, payload: Record<string, unknown>): SessionEvent =>
  ({
    id: `${sessionId}-${type}`,
    sessionId,
    type,
    timestamp: new Date(1_000),
    traceId: "trace",
    sequenceNumber: 1,
    payload,
  }) as unknown as SessionEvent;

// X 的持久化 transcript 里有一个 Agent tool part（嵌套 Explore）。
const childMessages = (status: "running" | "completed") => [
  {
    info: { id: "msg_x", role: "assistant", time: { created: 1 } },
    parts: [
      {
        type: "tool",
        tool: "Agent",
        callID: "call_explore",
        metadata: { childSessionId: GRANDCHILD },
        state: {
          status,
          input: { description: "Find scale entry points", subagent_type: "Explore" },
          ...(status === "completed"
            ? { output: "done", time: { start: 1_000, end: 2_000 } }
            : { time: { start: 1_000 } }),
        },
      },
    ],
  },
];

function contextWith(grandchildEvents: SessionEvent[], partStatus: "running" | "completed") {
  const sessions = new Map<string, unknown>();
  const events: Record<string, SessionEvent[]> = {
    [CHILD]: [
      event(CHILD, SessionEventType.SubagentSpawned, {
        parentToolCallId: "call_explore",
        childSessionId: GRANDCHILD,
        agentType: "Explore",
      }),
    ],
    [GRANDCHILD]: grandchildEvents,
  };
  // 只有主会话有 bootstrap record；X 与孙会话共享它的 event store。
  sessions.set(MAIN, {
    eventStore: { getEvents: async (id: string) => events[id] ?? [] },
    app: { runtime: { getProjection: async () => undefined } },
  });
  const stored: Record<string, unknown> = {
    [MAIN]: session(MAIN),
    [CHILD]: session(CHILD, MAIN),
    [GRANDCHILD]: session(GRANDCHILD, CHILD),
  };
  return {
    deps: {
      sessionStore: {
        getSession: async (id: string) => stored[id] ?? null,
        messages: async ({ sessionID }: { sessionID: string }) =>
          sessionID === CHILD ? childMessages(partStatus) : [],
      },
    },
    sessions,
  } as unknown as ZCodeProtocolAgentServerContext;
}

describe("session/subagents for a subagent child session", () => {
  it("reports a running nested Explore as running via the host event store", async () => {
    const context = contextWith([event(GRANDCHILD, SessionEventType.TurnStarted, {})], "running");
    const result = await listSessionSubagents(context, { sessionId: CHILD, endedLimit: 10 });
    expect(result.running.map((agent) => agent.childSessionId)).toEqual([GRANDCHILD]);
    expect(result.running[0]?.subagentType).toBe("Explore");
    expect(result.ended.total).toBe(0);
  });

  it("lists the nested Explore as ended once it stopped", async () => {
    const context = contextWith([], "completed");
    // 停止事件写在 X 的事件流里。
    const host = context.sessions.get(MAIN) as unknown as {
      eventStore: { getEvents: (id: string) => Promise<SessionEvent[]> };
    };
    const base = host.eventStore.getEvents;
    host.eventStore.getEvents = async (id: string) =>
      id === CHILD
        ? [
            ...(await base(id)),
            event(CHILD, SessionEventType.SubagentStopped, {
              parentToolCallId: "call_explore",
              childSessionId: GRANDCHILD,
              status: "completed",
            }),
          ]
        : base(id);
    const result = await listSessionSubagents(context, { sessionId: CHILD, endedLimit: 10 });
    expect(result.running).toEqual([]);
    expect(result.ended.items.map((agent) => agent.childSessionId)).toEqual([GRANDCHILD]);
  });
});
