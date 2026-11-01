// 冷投影保鲜只服务「事件由别的进程追加」的会话（docs/design/v2/tool/07-subagent.md
// 「立即发送时转后台」冷投影保鲜条）。回归 2026-11-01：父会话空闲、后台 subagent 仍在本进程
// 运行时，GPUI 尾部轮询每 3 s 强制重物化，冷种子清空 live subagents.running。
import { describe, expect, it } from "vitest";
import { isSessionRecordProducingLiveEvents } from "../src/zcode-protocol/v4-bridge.js";

function record(runtime: { foreground?: string; background?: boolean; aborter?: boolean }) {
  return {
    ...(runtime.aborter ? { activeAbortController: new AbortController() } : {}),
    app: {
      runtime: {
        getActiveForegroundExecutionId: () => runtime.foreground,
        hasRunningBackgroundTasks: () => runtime.background === true,
      },
    },
  };
}

describe("isSessionRecordProducingLiveEvents", () => {
  it("is false for an idle session with no background work, so external appends still refresh", () => {
    expect(isSessionRecordProducingLiveEvents(record({}))).toBe(false);
  });

  it("is true while a turn runs", () => {
    expect(isSessionRecordProducingLiveEvents(record({ aborter: true }))).toBe(true);
    expect(isSessionRecordProducingLiveEvents(record({ foreground: "fg-1" }))).toBe(true);
  });

  it("is true while an idle session still has running background tasks in this process", () => {
    expect(isSessionRecordProducingLiveEvents(record({ background: true }))).toBe(true);
  });

  it("tolerates runtime doubles without the optional methods", () => {
    expect(isSessionRecordProducingLiveEvents({ app: { runtime: {} } })).toBe(false);
  });
});
