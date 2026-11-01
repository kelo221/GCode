import { describe, expect, it } from "vitest";
import {
  ZCODE_SPEAK_MESSAGE_MAX_CHARS,
  zcodeProtocolMethods,
  zcodeSpeakParamsSchema,
  zcodeSpeakResultSchema,
} from "../src/zcode-protocol/index.js";

describe("interaction/speak", () => {
  it("validates params and results strictly", () => {
    expect(zcodeProtocolMethods.interactionSpeak).toBe("interaction/speak");
    expect(
      zcodeSpeakParamsSchema.parse({
        sessionId: "sess_1",
        message: " Plan ready ",
        kind: "planReady",
      }),
    ).toEqual({ sessionId: "sess_1", message: "Plan ready", kind: "planReady" });
    expect(() =>
      zcodeSpeakParamsSchema.parse({ sessionId: "sess_1", message: "   ", kind: "done" }),
    ).toThrow();
    expect(() =>
      zcodeSpeakParamsSchema.parse({
        sessionId: "sess_1",
        message: "a".repeat(ZCODE_SPEAK_MESSAGE_MAX_CHARS + 1),
        kind: "done",
      }),
    ).toThrow();
    expect(() =>
      zcodeSpeakParamsSchema.parse({ sessionId: "sess_1", message: "hi", kind: "shout" }),
    ).toThrow();
    expect(zcodeSpeakResultSchema.parse({ accepted: false, reason: "disabled" })).toEqual({
      accepted: false,
      reason: "disabled",
    });
    expect(() => zcodeSpeakResultSchema.parse({ accepted: true, extra: 1 })).toThrow();
  });
});
