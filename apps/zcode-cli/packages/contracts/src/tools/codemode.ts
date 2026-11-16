// codemode（docs/design/v2/tool/21-codemode.md）：脚本内调用工具。
import { z } from "zod";

export const CODEMODE_TOOL_NAME = "codemode";

export const CODEMODE_MODES = ["off", "on", "only"] as const;
export type CodemodeMode = (typeof CODEMODE_MODES)[number];

export const CODEMODE_DISPLAY_MAX_CALLS = 100;
export const CODEMODE_DISPLAY_MAX_MEDIA = 4;
/** 256 KiB 原图编码后的最大 base64 长度。 */
export const CODEMODE_DISPLAY_MAX_MEDIA_BASE64 = 349_528;

/**
 * codemode 工具行的嵌套调用清单。与 packages/shared/src/zcode-protocol-v4/toolDisplay.ts 的
 * codemode 成员必须同步——两侧都是 strict。
 */
export const codemodeToolResultDisplayPayloadSchema = z
  .object({
    kind: z.literal("codemode"),
    ok: z.boolean(),
    calls: z
      .array(
        z
          .object({
            name: z.string().min(1).max(256),
            status: z.enum(["ok", "error", "cancelled"]),
            durationMs: z.number().int().nonnegative(),
          })
          .strict(),
      )
      .max(CODEMODE_DISPLAY_MAX_CALLS),
    truncated: z.literal(true).optional(),
    // 脚本 image() 输出的图片，供 UI 显示；预算同 CUA display（单张 256 KiB 原图）。
    media: z
      .array(
        z
          .object({
            mimeType: z.string().regex(/^image\/[a-z0-9.+-]+$/iu),
            data: z.string().min(1).max(CODEMODE_DISPLAY_MAX_MEDIA_BASE64),
          })
          .strict(),
      )
      .min(1)
      .max(CODEMODE_DISPLAY_MAX_MEDIA)
      .optional(),
  })
  .strict();

export type CodemodeToolResultDisplayPayload = z.infer<
  typeof codemodeToolResultDisplayPayloadSchema
>;
