// Fork: the codemode tool's display lists the script's nested tool calls
// (apps/zcode-cli/docs/design/v2/tool/21-codemode.md). The V4 mirror must accept the same
// shape as the CLI contracts member (codemodeToolResultDisplayPayloadSchema), or the GPUI card
// loses its call list on the wire.
import { describe, expect, it } from "vitest";
import { zcodeSessionRuntimePreferencesResultSchema } from "../src/zcode-protocol/index.js";
import { toolOutputSchema } from "../src/zcode-protocol-v4/index.js";

const display = {
  kind: "codemode",
  ok: false,
  calls: [
    { name: "Read", status: "ok", durationMs: 12 },
    { name: "mcp__blender__render", status: "error", durationMs: 0 },
    { name: "Bash", status: "cancelled", durationMs: 3 },
  ],
  truncated: true,
};

describe("codemode display", () => {
  it("survives the V4 tool output schema", () => {
    expect(toolOutputSchema.parse({ text: "ok", display }).display).toEqual(display);
  });

  it("drops a display with an unknown field instead of failing the output", () => {
    const parsed = toolOutputSchema.parse({ text: "ok", display: { ...display, extra: 1 } });
    expect(parsed.display).toBeUndefined();
    expect(parsed.text).toBe("ok");
  });

  it("carries codemodeMode in runtime preferences", () => {
    const base = { nativeSearchEnhancementsEnabled: true };
    expect(
      zcodeSessionRuntimePreferencesResultSchema.parse({ ...base, codemodeMode: "only" })
        .codemodeMode,
    ).toBe("only");
    expect(zcodeSessionRuntimePreferencesResultSchema.parse(base).codemodeMode).toBeUndefined();
    expect(() =>
      zcodeSessionRuntimePreferencesResultSchema.parse({ ...base, codemodeMode: "auto" }),
    ).toThrow();
  });
});
