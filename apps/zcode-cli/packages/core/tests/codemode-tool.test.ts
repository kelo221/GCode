/**
 * codemode（docs/design/v2/tool/21-codemode.md）：脚本里的 `tools.*` 走完整执行管线。
 * 钉住：结果脱敏、plan mode 权限、禁止项、事件标记（无 Scheduled、带 source/parent）、
 * 非并发安全调用串行、store 增量与重建、`only` 模式暴露。
 */
import { afterEach, describe, expect, it } from "vitest";
import {
  SessionEventType,
  codemodeToolResultDisplayPayloadSchema,
  createRootTraceContext,
  createSessionId,
  createToolCallId,
  createTurnId,
  type CollaborationMode,
  type MessageWithParts,
  type ModelToolContract,
  type SessionEvent,
} from "@zcode/contracts";
import { PermissionService, defaultPermissionConfig } from "../src/permission/service.js";
import { replaceSecretVault } from "../src/secrets/secret-vault.js";
import { createToolExecutor } from "../src/tool/executor.js";
import {
  codemodeToolEntry,
  createCodemodeDisplay,
  trimToBudget,
  type CodemodeOutput,
} from "../src/tool/handlers/codemode.js";
import { createToolRegistry } from "../src/tool/registry.js";
import type { ToolEntry } from "../src/tool/types.js";
import { hydrateCodemodeStoreFromSession } from "../src/agent/codemode-store-hydrator.js";
import { applyCodemodeOnlyExposure } from "../src/tool/codemode/catalog.js";
import { runCodemodeScript } from "../src/tool/codemode/sandbox.js";
import { prepareRuntimeProviderRequest } from "../src/runtime/helpers/runtime-provider-request-messages.js";
import { toolSearchToolEntry } from "../src/tool/handlers/tool-search.js";
import { deferredToolCallEntry } from "../src/tool/handlers/deferred-tool-call.js";
import { createTestRuntimeModel } from "./test-runtime-model.js";

const SECRET = "s3cr3t-token-value";

function probe(
  name: string,
  handler: ToolEntry["handler"],
  overrides: Partial<ToolEntry["metadata"]> = {},
): ToolEntry {
  const readOnly = overrides.readOnly ?? true;
  const sideEffectScope = overrides.sideEffectScope ?? (readOnly ? "none" : "workspace");
  return {
    capability: `probe ${name}`,
    metadata: {
      name,
      description: `${name} probe tool`,
      readOnly,
      destructive: false,
      concurrentSafe: readOnly,
      sideEffectScope,
      riskLevel: "low",
      needsApproval: false,
      ...overrides,
    },
    handler,
    inputSchema: { type: "object" },
    outputSchema: { type: "object" },
    formatModelContent: (output) => String((output as { text: string }).text),
    permission: {
      permission: name,
      reason: "probe",
      riskLevel: "low",
      sideEffectScope,
      needsApproval: false,
      patternSources: ["toolName"],
      denyPriority: "beforeAsk",
    },
    resultBudget: { maxInlineBytes: 10_000, maxModelBytes: 10_000, strategy: "truncate" },
    timeout: { defaultMs: 5_000, maxMs: 5_000, allowCallOverride: false },
    cancellation: { supported: true, cleanup: "none", userVisibleMessage: "cancelled" },
    trace: {
      required: true,
      propagateToAdapters: false,
      recordInput: "summary",
      recordOutput: "summary",
    },
  };
}

function setup(options: {
  mode?: CollaborationMode;
  tools?: ToolEntry[];
  store?: Map<string, unknown>;
}) {
  const sessionId = createSessionId("codemode");
  const turnId = createTurnId("codemode");
  const traceContext = createRootTraceContext({ sessionId, turnId });
  const registry = createToolRegistry();
  registry.register(codemodeToolEntry);
  for (const tool of options.tools ?? []) registry.register(tool);
  const events: SessionEvent[] = [];
  const executor = createToolExecutor({
    emitEvent: async (event) => {
      events.push(event);
    },
    mode: options.mode ?? "yolo",
    permissionBroker: {
      async requestPermission() {
        return { decision: "allow" as const };
      },
    },
    permissionService: new PermissionService(defaultPermissionConfig),
    registry,
    sessionId,
    turnId,
    traceContext,
    workingDirectory: "/probe/cwd",
    codemodeStore: options.store,
  });
  const run = async (code: string, signal?: AbortSignal) => {
    const id = createToolCallId("cm");
    const result = await executor.execute(
      { id, input: { code }, name: "codemode" },
      { traceContext, ...(signal ? { signal } : {}) },
    );
    return { id, result, output: result.output as CodemodeOutput };
  };
  return { events, run };
}

const echo = probe("Echo", async (input) => ({
  text: `echo:${(input as { value: string }).value}`,
}));

describe("codemode tool", () => {
  afterEach(() => replaceSecretVault([]));

  it("runs nested calls through the executor and returns only the script output", async () => {
    const { run, events } = setup({ tools: [echo] });
    const { id, result, output } = await run(`
      const [a, b] = await Promise.all([tools.Echo({ value: "a" }), tools.Echo({ value: "b" })]);
      return a + "|" + b;
    `);
    expect(result.success).toBe(true);
    expect(output.ok).toBe(true);
    expect(output.text).toContain("Script completed");
    expect(output.text).toContain("echo:a|echo:b");
    expect(output.calls.map((call) => call.status)).toEqual(["ok", "ok"]);

    const nested = events.filter(
      (event) =>
        event.type === SessionEventType.ToolCallStarted &&
        String((event.payload as { toolCallId: string }).toolCallId).startsWith(`${id}~cm`),
    );
    expect(nested).toHaveLength(2);
    for (const event of nested) {
      expect(event.payload).toMatchObject({ source: "codemode", parentToolCallId: id });
    }
    expect(events.some((event) => event.type === SessionEventType.ToolCallScheduled)).toBe(false);
    expect(result.display).toMatchObject({ kind: "codemode", ok: true });
  });

  it("redacts vault secrets before the script sees a nested result", async () => {
    replaceSecretVault([{ name: "TOKEN", value: SECRET }]);
    const leaky = probe("Leaky", async () => ({ text: `token=${SECRET}` }));
    const { run } = setup({ tools: [leaky] });
    const { output } = await run(`return await tools.Leaky({});`);
    expect(output.text).toContain("token=***");
    expect(output.text).not.toContain(SECRET);
  });

  it("applies plan mode to nested calls: writes are denied, reads pass", async () => {
    const writer = probe("Writer", async () => ({ text: "wrote" }), { readOnly: false });
    const { run } = setup({ mode: "plan", tools: [echo, writer] });
    const { result, output } = await run(`
      const read = await tools.Echo({ value: "x" });
      const write = await tools.Writer({}).then(() => "allowed", (error) => "denied: " + error.message);
      return { read, write };
    `);
    expect(result.success).toBe(true);
    expect(output.text).toContain("echo:x");
    expect(output.text).toMatch(/denied: .*Plan mode/u);
  });

  it.each(["AskUserQuestion", "Speak"])(
    "rejects %s, which cannot be called from a script",
    async (name) => {
      let called = false;
      const blocked = probe(name, async () => {
        called = true;
        return { text: "never" };
      });
      const { run } = setup({ tools: [blocked] });
      const { output } = await run(`
      const listed = ALL_TOOLS.some((tool) => tool.name === "${name}");
      let call;
      try { await tools.${name}({}); call = "called"; } catch (error) { call = "rejected"; }
      return { listed, call };
    `);
      expect(output.text).toContain('"listed": false');
      expect(output.text).toContain('"call": "rejected"');
      expect(called).toBe(false);
    },
  );

  it("rejects turn-ending and hidden tools by metadata", async () => {
    const ender = probe("Ender", async () => ({ text: "never" }), { stopTurnOnSuccess: true });
    const hidden = probe("Hidden", async () => ({ text: "never" }), { providerVisible: false });
    const { run } = setup({ tools: [ender, hidden] });
    const { output } = await run(`return ALL_TOOLS.map((tool) => tool.name).join(",");`);
    expect(output.text).not.toContain("Ender");
    expect(output.text).not.toContain("Hidden");
  });

  it("serializes nested calls that are not concurrency safe", async () => {
    let active = 0;
    let maxActive = 0;
    const slow = probe(
      "SlowWrite",
      async () => {
        active += 1;
        maxActive = Math.max(maxActive, active);
        await new Promise((resolve) => setTimeout(resolve, 20));
        active -= 1;
        return { text: "ok" };
      },
      { readOnly: false },
    );
    const { run } = setup({ tools: [slow] });
    const { output } = await run(
      `await Promise.all([1, 2, 3].map(() => tools.SlowWrite({}))); return "done";`,
    );
    expect(output.ok).toBe(true);
    expect(maxActive).toBe(1);
  });

  it("cancels the script and pending nested calls when the turn aborts", async () => {
    const hang = probe(
      "Hang",
      (_input, context) =>
        new Promise((_resolve, reject) => {
          context.abortSignal.addEventListener("abort", () => reject(new Error("aborted")));
        }),
    );
    const { run } = setup({ tools: [hang] });
    const controller = new AbortController();
    const pending = run(`await tools.Hang({}); return "unreachable";`, controller.signal);
    setTimeout(() => controller.abort(), 200);
    const { result } = await pending;
    expect(result.success).toBe(false);
  });

  it("does not start queued nested calls after the turn aborts", async () => {
    let queuedStarted = false;
    const hang = probe(
      "HangWrite",
      (_input, context) =>
        new Promise((_resolve, reject) => {
          context.abortSignal.addEventListener("abort", () => reject(new Error("aborted")));
        }),
      { readOnly: false },
    );
    const queued = probe(
      "QueuedWrite",
      async () => {
        queuedStarted = true;
        return { text: "ran" };
      },
      { readOnly: false },
    );
    const { run } = setup({ tools: [hang, queued] });
    const controller = new AbortController();
    const pending = run(
      `await Promise.allSettled([tools.HangWrite({}), tools.QueuedWrite({})]); return "end";`,
      controller.signal,
    );
    setTimeout(() => controller.abort(), 200);
    await pending;
    expect(queuedStarted).toBe(false);
  });

  it("keeps store values across calls and reports the delta", async () => {
    const store = new Map<string, unknown>();
    const { run } = setup({ store });
    const first = await run(`store("cursor", 41); return "saved";`);
    expect(first.output.storeWrites).toEqual({ set: { cursor: 41 }, delete: [] });
    expect(store.get("cursor")).toBe(41);
    const second = await run(`return load("cursor") + 1;`);
    expect(second.output.text).toContain("42");
    const failed = await run(`store("cursor", 0); throw new Error("boom");`);
    expect(failed.output.ok).toBe(false);
    expect(failed.output.text).toContain("Script error: Error: boom");
    expect(store.get("cursor")).toBe(41);
  });

  it("never returns the untrimmed text for a zero output budget", async () => {
    const text = "x".repeat(500);
    expect(trimToBudget(text, 0)).toBe(
      "[... 500 characters omitted; print less or raise max_output_tokens ...]",
    );
    expect(trimToBudget(text, 1)).not.toContain("xxx");
    const { output } = await setup({}).run(
      `// @options: {"max_output_tokens": 0}
return "${"y".repeat(200)}";`,
    );
    expect(output.text).not.toContain("yyy");
    expect(output.text).toContain("characters omitted");
  });

  it("keeps the display valid when a call has an empty name", () => {
    const display = createCodemodeDisplay({
      ok: true,
      text: "",
      images: [],
      calls: [{ name: "", status: "ok", durationMs: 1.6 }],
      durationMs: 2,
    } satisfies CodemodeOutput);
    expect(display?.calls).toEqual([{ name: "unknown", status: "ok", durationMs: 2 }]);
  });

  it("puts script images in the display media within the size budget", () => {
    const small = "QUJD";
    // 300 KiB 原图：超过单张 256 KiB 上限，只给模型。
    const huge = "A".repeat(Math.ceil((300 * 1024 * 4) / 3));
    const output = {
      ok: true,
      text: "",
      images: [
        { mimeType: "image/jpeg", data: small },
        { mimeType: "image/png", data: huge },
        { mimeType: "text/plain", data: small },
        { mimeType: "image/png", data: small },
      ],
      calls: [],
      durationMs: 1,
    } satisfies CodemodeOutput;
    const display = createCodemodeDisplay(output);
    expect(display?.media).toEqual([
      { mimeType: "image/jpeg", data: small },
      { mimeType: "image/png", data: small },
    ]);
    expect(codemodeToolResultDisplayPayloadSchema.parse(display)).toEqual(display);
    expect(createCodemodeDisplay({ ...output, images: [] })).not.toHaveProperty("media");
  });

  it("returns the worker slot when the sandbox cannot be created", async () => {
    const tool = { name: "dup", execute: async () => "x" };
    const options = { signal: new AbortController().signal };
    // 进程最多 2 个槽位：构造失败若不归还，第三次就会卡在排队上。
    for (let attempt = 0; attempt < 3; attempt++) {
      await expect(
        runCodemodeScript({ code: "return 1;", tools: [tool, tool], globals: [], options }),
      ).rejects.toThrow();
    }
    const ok = await runCodemodeScript({ code: "return 1;", tools: [], globals: [], options });
    expect(ok.ok).toBe(true);
  });

  it("reports invalid options lines as a tool failure", async () => {
    const { run } = setup({});
    const { result } = await run(`return 1;\n// @options: {"timeout_ms": 5}`);
    expect(result.success).toBe(false);
    expect(result.error?.message).toContain("invalid_source");
  });
});

describe("codemode store hydration", () => {
  function message(id: string, parts: unknown[]): MessageWithParts {
    return {
      info: { id, role: "assistant", sessionID: "s", time: { created: 0 } },
      parts,
    } as unknown as MessageWithParts;
  }
  function part(id: string, codemodeStore: unknown) {
    return {
      id,
      type: "tool",
      tool: "codemode",
      callID: id,
      state: {
        status: "completed",
        input: {},
        output: "",
        metadata: { schemaVersion: 1, codemodeStore },
      },
    };
  }

  it("replays deltas in history order", () => {
    const store = new Map<string, unknown>([["stale", true]]);
    const applied = hydrateCodemodeStoreFromSession({
      store,
      messages: [
        message("m1", [part("p1", { set: { a: 1, b: 2 }, delete: [] })]),
        message("m2", [part("p2", { set: { a: 3 }, delete: ["b"] })]),
      ],
    });
    expect(applied).toBe(2);
    expect(Object.fromEntries(store)).toEqual({ a: 3 });
  });

  it("drops writes made after the rewind target", () => {
    const store = new Map<string, unknown>();
    hydrateCodemodeStoreFromSession({
      store,
      messages: [
        message("m1", [part("p1", { set: { a: 1 }, delete: [] })]),
        message("m2", [part("p2", { set: { a: 2, b: 2 }, delete: [] })]),
      ],
      rewindTargetMessageId: "m2" as never,
    });
    expect(Object.fromEntries(store)).toEqual({ a: 1 });
  });
});

describe("codemode only mode", () => {
  const contract = (name: string, description = `${name} tool`): ModelToolContract =>
    ({
      name,
      description,
      inputSchema: { type: "object", properties: { path: { type: "string" } } },
    }) as ModelToolContract;

  it("keeps codemode and turn-control tools and declares the rest in the codemode description", () => {
    const tools = applyCodemodeOnlyExposure([
      contract("codemode", "Run a script."),
      contract("Read"),
      contract("Bash"),
      contract("AskUserQuestion"),
      contract("TodoWrite"),
    ]);
    expect(tools.map((tool) => tool.name)).toEqual(["codemode", "AskUserQuestion", "TodoWrite"]);
    const description = tools[0]?.description ?? "";
    expect(description).toContain("Read(");
    expect(description).toContain("Bash(");
  });

  it.each(["local", "native"] as const)(
    "turns tool search off so deferred tools stay declared (toolSearch=%s)",
    (mode) => {
      const registry = createToolRegistry();
      registry.register(codemodeToolEntry);
      registry.register(toolSearchToolEntry);
      registry.register(deferredToolCallEntry);
      registry.register(
        probe("mcp__probe__thing", async () => ({ text: "ok" }), { shouldDefer: true }),
      );
      const model = {
        ...createTestRuntimeModel({
          generateText: async () => ({ text: "", finishReason: "stop", usage: {} }),
        }),
        requestCapabilities: { toolSearch: { protocolSupported: true, defaultEnabled: true } },
      };
      const prepared = prepareRuntimeProviderRequest(
        { config: { toolSearch: { mode }, codemode: { mode: "only" } } },
        { entries: [], registry, model, tools: registry.toContracts() },
      );
      expect(prepared.request.tools.map((tool) => tool.name)).toEqual(["codemode"]);
      expect(prepared.request.toolSearchEnabled).toBe(false);
      expect(prepared.request.tools.some((tool) => tool.deferLoading)).toBe(false);
      expect(prepared.request.tools[0]?.description).toContain("mcp__probe__thing");
      expect(prepared.toolSearch.enabled).toBe(false);
    },
  );
});
