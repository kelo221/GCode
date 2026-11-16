// ============================================================
// Speak Tool - 把一句简短的话读给用户听（GPUI 桌面专属）
// ============================================================
//
// 只在 Host 开启 voice 的主会话里注册（见 VoicePort）。工具在客户端接受后立即返回，
// 不等待播放结束。

import { z } from "zod";
import { toToolJsonSchema } from "./json-schema.js";

export const SPEAK_TOOL_NAME = "Speak";
export const SPEAK_MESSAGE_MAX_CHARS = 300;

export const SpeakInputSchema = z
  .object({
    message: z.string().trim().min(1).max(SPEAK_MESSAGE_MAX_CHARS),
    kind: z.enum(["attention", "done", "planReady", "info"]),
  })
  .strict();

export type SpeakInput = z.infer<typeof SpeakInputSchema>;

export const SpeakInputJsonSchema = toToolJsonSchema(SpeakInputSchema);

export const SpeakOutputSchema = z
  .object({
    spoken: z.boolean(),
    // tooSoon 由 CLI 自己判定（info 播报节流），不经过客户端。
    reason: z.enum(["disabled", "unfocusedOnly", "unavailable", "tooSoon"]).optional(),
  })
  .strict();

export type SpeakOutput = z.infer<typeof SpeakOutputSchema>;

export const SpeakOutputJsonSchema = toToolJsonSchema(SpeakOutputSchema);
