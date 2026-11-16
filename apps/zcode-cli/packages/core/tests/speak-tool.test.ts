import { describe, expect, it, vi } from "vitest";
import type { TraceId, VoicePort } from "@zcode/contracts";
import { createToolRegistry, registerBuiltInTools } from "../src/tool/index.js";
import { PermissionService } from "../src/permission/service.js";
import { resolveRuntimePermissionCapability } from "../src/tool/executor/permission-capability.js";
import {
  SPEAK_INFO_MIN_WORK_STEPS,
  SpeakCadence,
  speakToolEntry,
} from "../src/tool/handlers/speak.js";
import type { ToolExecutionContext } from "../src/tool/types.js";

function createContext(port?: VoicePort): ToolExecutionContext {
  return {
    toolCallId: "tool-speak",
    traceId: "trace-speak" as TraceId,
    abortSignal: new AbortController().signal,
    voicePort: port,
    workingDirectory: "/workspace",
    workspaceRoot: "/workspace",
    sessionId: "sess-speak" as never,
  };
}

describe("Speak tool", () => {
  it("is registered only with includeSpeak", () => {
    const without = createToolRegistry();
    registerBuiltInTools(without, {});
    expect(without.has("Speak")).toBe(false);

    const withFlag = createToolRegistry();
    registerBuiltInTools(withFlag, { includeSpeak: true });
    expect(withFlag.has("Speak")).toBe(true);
  });

  it("exposes provider-compatible JSON schemas", () => {
    for (const schema of [speakToolEntry.inputSchema, speakToolEntry.outputSchema]) {
      const text = JSON.stringify(schema);
      for (const key of ["anyOf", "$ref", "$defs", "definitions"]) {
        expect(text).not.toContain(`"${key}"`);
      }
    }
    expect(speakToolEntry.inputSchema).toMatchObject({
      type: "object",
      required: ["message", "kind"],
      additionalProperties: false,
    });
  });

  it("needs no approval and tells the model to keep it short and rare", () => {
    expect(speakToolEntry.metadata.needsApproval).toBe(false);
    expect(speakToolEntry.permission.needsApproval).toBe(false);
    const instructions = speakToolEntry.metadata.modelInstructions?.join("\n") ?? "";
    expect(instructions).toContain("one plain spoken sentence");
    expect(instructions).toContain("Do not narrate progress");
  });

  it("runs without a permission prompt in every mode", () => {
    const capability = resolveRuntimePermissionCapability(
      speakToolEntry,
      { message: "Task done", kind: "done" },
      { workingDirectory: "/workspace", workspaceRoot: "/workspace" },
    );
    const service = new PermissionService();
    for (const mode of ["build", "edit", "plan", "yolo", "guarded"] as const) {
      const decision = service.checkPermission(
        {
          input: { message: "Task done", kind: "done" },
          mode,
          riskLevel: "low",
          toolName: "Speak",
        },
        capability,
      );
      expect(decision, mode).toMatchObject({ allowed: true, decision: "allow" });
    }
  });

  it("forwards the message and kind and reports success", async () => {
    const speak = vi.fn(async () => ({ accepted: true as const }));
    const output = await speakToolEntry.handler(
      { message: "  Plan ready for review ", kind: "planReady" },
      createContext({ speak }),
    );
    expect(speak).toHaveBeenCalledWith(
      { message: "Plan ready for review", kind: "planReady" },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(output).toEqual({ spoken: true });
    expect(speakToolEntry.formatModelContent?.(output)).toBe("Spoken aloud to the user.");
  });

  it("reports rejections with a reason the model can act on", async () => {
    const output = await speakToolEntry.handler(
      { message: "Done", kind: "done" },
      createContext({ speak: async () => ({ accepted: false, reason: "disabled" }) }),
    );
    expect(output).toEqual({ spoken: false, reason: "disabled" });
    expect(speakToolEntry.formatModelContent?.(output)).toContain("Don't call Speak again");
  });

  it("fails cleanly without a voice port", async () => {
    const output = await speakToolEntry.handler(
      { message: "Done", kind: "done" },
      createContext(undefined),
    );
    expect(output).toMatchObject({
      result: false,
      message: expect.stringContaining("voice_unavailable"),
    });
  });

  it("rate-limits info heads-ups within a turn but never other kinds", async () => {
    const speak = vi.fn(async () => ({ accepted: true as const }));
    const cadence = new SpeakCadence();
    const ctx = { ...createContext({ speak }), speakCadence: cadence, turnId: "turn-1" as never };
    const info = { message: "Heads up, carrying on", kind: "info" };

    expect(await speakToolEntry.handler(info, ctx)).toEqual({ spoken: true });
    const tooSoon = await speakToolEntry.handler(info, ctx);
    expect(tooSoon).toEqual({ spoken: false, reason: "tooSoon" });
    expect(speakToolEntry.formatModelContent?.(tooSoon)).toContain("Keep working");
    // 收尾的 done 不受节流。
    expect(await speakToolEntry.handler({ message: "Done", kind: "done" }, ctx)).toEqual({
      spoken: true,
    });

    // 带 info 的步骤不攒名额；之后足够多的工作步才放行。
    cadence.recordWorkStep("turn-1", true);
    for (let step = 1; step < SPEAK_INFO_MIN_WORK_STEPS; step += 1) {
      cadence.recordWorkStep("turn-1", false);
    }
    expect(await speakToolEntry.handler(info, ctx)).toMatchObject({ reason: "tooSoon" });
    cadence.recordWorkStep("turn-1", false);
    expect(await speakToolEntry.handler(info, ctx)).toEqual({ spoken: true });

    // 新回合重置。
    const nextTurn = { ...ctx, turnId: "turn-2" as never };
    expect(await speakToolEntry.handler(info, nextTurn)).toEqual({ spoken: true });
    expect(speak).toHaveBeenCalledTimes(4);
  });

  it("rejects empty, overlong or unknown-kind input", async () => {
    const ctx = createContext({ speak: async () => ({ accepted: true }) });
    await expect(speakToolEntry.handler({ message: "  ", kind: "done" }, ctx)).rejects.toThrow();
    await expect(
      speakToolEntry.handler({ message: "a".repeat(301), kind: "done" }, ctx),
    ).rejects.toThrow();
    await expect(speakToolEntry.handler({ message: "hi", kind: "yell" }, ctx)).rejects.toThrow();
  });
});
