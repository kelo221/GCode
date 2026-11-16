// ============================================================
// Speak Tool Handler
// ============================================================
// 把一句简短的话交给客户端读出来（GPUI 桌面专属，见 apps/zcode-gpui/VOICE.md）。
// 端口在场即注册；客户端接受后立即返回，不等待播放结束。

import {
  SPEAK_TOOL_NAME,
  SpeakInputJsonSchema,
  SpeakInputSchema,
  SpeakOutputJsonSchema,
  SpeakOutputSchema,
  type ModelMessageContent,
  type SpeakOutput,
} from "@zcode/contracts";
import type { ToolEntry, ToolHandler, ToolHandlerFailure } from "../types.js";

const SPEAK_TIMEOUT_MS = 10_000;
const SPEAK_MODEL_BYTES = 1_000;
const SPEAK_ERROR_CODE = { VOICE_UNAVAILABLE: 41 } as const;

const SPEAK_DESCRIPTION =
  "Say one short sentence aloud to the user on their PC. Use it to get the user's attention when you need them, when a task is done, or when a plan is ready for review, or for a rare heads-up while you keep working.";

/** Tool steps without an info Speak that must finish before the next info heads-up in a turn. */
export const SPEAK_INFO_MIN_WORK_STEPS = 3;

/**
 * 回合中途的 info 播报节流（apps/zcode-gpui/VOICE.md「Mid-turn heads-ups」）。runtime 持有唯一实例：
 * handler 只在这里抢占名额，turn-tools 在每个含非 Speak 工具的步骤结束后记一步。
 * 只管 info：attention/done/planReady 是用户必须听到的，不受限。
 */
export class SpeakCadence {
  private turnId: string | undefined;
  private workStepsSinceInfo = 0;

  /** Claims the info slot for this turn; false while too few work steps passed since the last one. */
  tryClaimInfo(turnId: string | undefined): boolean {
    if (turnId === undefined) return true;
    if (turnId !== this.turnId) {
      this.turnId = turnId;
      this.workStepsSinceInfo = 0;
      return true;
    }
    if (this.workStepsSinceInfo < SPEAK_INFO_MIN_WORK_STEPS) return false;
    this.workStepsSinceInfo = 0;
    return true;
  }

  /** Counts a finished tool step that ran a non-Speak tool. */
  recordWorkStep(turnId: string | undefined, stepHadInfoSpeak: boolean): void {
    if (turnId === undefined || turnId !== this.turnId || stepHadInfoSpeak) return;
    this.workStepsSinceInfo += 1;
  }
}

const speakHandler: ToolHandler = async (input, context) => {
  const parsed = SpeakInputSchema.parse(input);
  const port = context.voicePort;
  if (port === undefined) {
    return {
      result: false,
      errorCode: SPEAK_ERROR_CODE.VOICE_UNAVAILABLE,
      message: "voice_unavailable: this session has no voice output.",
    } satisfies ToolHandlerFailure;
  }
  if (
    parsed.kind === "info" &&
    context.speakCadence &&
    !context.speakCadence.tryClaimInfo(context.turnId ?? context.traceContext?.turnId)
  ) {
    return { spoken: false, reason: "tooSoon" } satisfies SpeakOutput;
  }
  const outcome = await port.speak(
    { message: parsed.message, kind: parsed.kind },
    { signal: context.abortSignal },
  );
  return (
    outcome.accepted ? { spoken: true } : { spoken: false, reason: outcome.reason }
  ) satisfies SpeakOutput;
};

function formatSpeakModelContent(output: unknown): ModelMessageContent {
  const parsed = SpeakOutputSchema.safeParse(output);
  if (!parsed.success) return "Speak returned an invalid result.";
  if (parsed.data.spoken) return "Spoken aloud to the user.";
  switch (parsed.data.reason) {
    case "disabled":
      return "Not spoken: the user turned voice off. Don't call Speak again in this session.";
    case "unfocusedOnly":
      return "Not spoken: the user is looking at GCode right now, so voice stayed silent.";
    case "tooSoon":
      return "Not spoken: you gave a heads-up only a few steps ago. Keep working; the user will see your written reply.";
    default:
      return "Not spoken: voice output is unavailable. Continue without it.";
  }
}

export const speakToolEntry: ToolEntry = {
  capability: "Say one short sentence aloud to the user",
  metadata: {
    name: SPEAK_TOOL_NAME,
    description: SPEAK_DESCRIPTION,
    modelInstructions: [
      "Call Speak only when the user's attention matters: you need an answer or a permission, you are blocked, a task is done, or a plan is ready (kind: attention, done, planReady; info for anything else worth hearing).",
      "Say one plain spoken sentence, ideally under 15 words. No code, file paths, URLs, markdown or lists: it is read aloud.",
      "Mid-turn heads-up: to tell the user something worth hearing now and keep working (a decision they may want to override, a long step finished), call Speak with kind info in the same response as your next work tool call. Info heads-ups are rate-limited; a rejected one returns tooSoon.",
      "Do not narrate progress, and do not call Speak twice in a row or for every step. One call at the end of a task is the normal case.",
      "Speak is in addition to your written reply, never a replacement for it.",
      "To close a turn, write your complete final reply first and call Speak in that same response. A response with reply text whose only tool calls are Speak ends the turn right there, with no further step, so put nothing you still need to say after the call.",
    ],
    readOnly: true,
    destructive: false,
    concurrentSafe: true,
    timeoutMs: SPEAK_TIMEOUT_MS,
    maxOutputBytes: SPEAK_MODEL_BYTES,
    // 修复原因：`userInteraction` 会让 PermissionService 在任何模式下都直接 ask（规则
    // tool.userInteraction，且先于 yolo 与 always-allow 判断），所以每次 Speak 都弹审批、
    // 「总是允许」也不生效。Speak 只把一句话交回客户端，和 submit_result 一样属于 `session`。
    sideEffectScope: "session",
    riskLevel: "low",
    needsApproval: false,
  },
  handler: speakHandler,
  inputSchema: SpeakInputJsonSchema,
  outputSchema: SpeakOutputJsonSchema,
  runtimeInputSchema: SpeakInputSchema,
  runtimeOutputSchema: SpeakOutputSchema,
  formatModelContent: formatSpeakModelContent,
  permission: {
    permission: "speak",
    reason: "Speak plays a short sentence through the user's speakers",
    riskLevel: "low",
    sideEffectScope: "session",
    needsApproval: false,
    patternSources: ["toolName"],
    alwaysAllowPatternSources: ["toolName"],
    denyPriority: "beforeAsk",
  },
  resultBudget: {
    maxInlineBytes: SPEAK_MODEL_BYTES,
    maxModelBytes: SPEAK_MODEL_BYTES,
    strategy: "truncate",
    preview: { maxBytes: SPEAK_MODEL_BYTES, direction: "head" },
  },
  timeout: {
    kind: "timed",
    defaultMs: SPEAK_TIMEOUT_MS,
    maxMs: SPEAK_TIMEOUT_MS,
    allowCallOverride: false,
  },
  cancellation: {
    supported: true,
    cleanup: "none",
    userVisibleMessage: "Speak was cancelled",
  },
  trace: {
    required: true,
    propagateToAdapters: false,
    recordInput: "summary",
    recordOutput: "summary",
  },
};
