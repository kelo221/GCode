import { afterEach, describe, expect, it } from "vitest";
import { withTimeout } from "../src/mcp/timeout.js";

describe("withTimeout", () => {
  const seen: unknown[] = [];
  const onUnhandled = (reason: unknown) => seen.push(reason);
  afterEach(() => {
    process.off("unhandledRejection", onUnhandled);
    seen.length = 0;
  });

  // 2026-11-07: an already-aborted signal left client.connect() without a handler; its later
  // rejection (SDK closes the transport) became an unhandledRejection that killed the backend.
  it("swallows a late rejection of the abandoned promise when the signal is already aborted", async () => {
    process.on("unhandledRejection", onUnhandled);
    const controller = new AbortController();
    controller.abort(new Error("probe stopped"));
    let rejectLater!: (error: Error) => void;
    const connect = new Promise<void>((_, reject) => {
      rejectLater = reject;
    });

    await expect(withTimeout(connect, 1_000, "timed out", controller.signal)).rejects.toThrow(
      "probe stopped",
    );
    rejectLater(new Error("Version negotiation failed: the transport was closed"));
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(seen).toEqual([]);
  });
});
