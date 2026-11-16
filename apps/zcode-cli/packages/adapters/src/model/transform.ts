// ============================================================
// Vercel AI SDK protocol transforms
// ============================================================

import { type ModelMessage as AiSdkModelMessage, type ToolResultPart } from "ai";
import {
  ModelErrorCode,
  isBlockIndex,
  modelMessageContentToText,
  type ModelCacheControl,
  type ModelInputMessage,
  type ModelInputFormat,
  type ModelMessageContent,
  type ModelMessageContentBlock,
} from "@zcode/contracts";
import { AiSdkModelAdapterError } from "./errors.js";
import { providerOptionsForReasoningBlock } from "./anthropic-reasoning-metadata.js";
import {
  shouldTextifyStructuredToolResults,
  toStructuredToolResultText,
  undeliverableFrameReferenceText,
  toToolResultMediaUserParts,
  toolResultHasVideoMedia,
} from "./tool-result-media-projection.js";
import {
  dataUrlToDataContent,
  providerImageMediaType,
  unsupportedImageMediaTypeText,
  unsupportedInputMediaText,
} from "./media-transform-policy.js";
import { normalizeOpenAiCompatibleSystemMessages } from "./system-message-compat.js";

export interface AiSdkMessageTransformOptions {
  toolSearchEnabled?: boolean;
  apiFormat?: string;
  providerOptions?: Record<string, unknown>;
  providerKind?: "openai" | "anthropic" | "openai-compatible" | "gateway" | "custom";
  stripMedia?: boolean;
  inputFormat?: ModelInputFormat;
  /** 本次请求的目标模型；用于判断历史 tool call 的 provider 元数据是否属于同一模型。 */
  targetModel?: { providerId: string; modelId: string };
}

type AiSdkAssistantMessage = Extract<AiSdkModelMessage, { role: "assistant" }>;
type AiSdkAssistantContent = AiSdkAssistantMessage["content"];
type AiSdkAssistantContentPart = Extract<AiSdkAssistantContent, unknown[]>[number];
type AiSdkToolResultOutput = ToolResultPart["output"];
type AiSdkProviderOptions = NonNullable<
  Extract<AiSdkModelMessage, { role: "system" }>["providerOptions"]
>;
type AiSdkAssistantTransformOptions = AiSdkMessageTransformOptions & {
  stripOpenAiResponsesStoredReasoning?: boolean;
  replayToolCallThoughtSignature?: boolean;
};

export function toAiSdkMessages(
  messages: ModelInputMessage[],
  options: AiSdkMessageTransformOptions = {},
): AiSdkModelMessage[] {
  const normalizedMessages =
    options.providerKind === "openai-compatible"
      ? normalizeOpenAiCompatibleSystemMessages(messages)
      : messages;
  const shouldStripOpenAiResponsesStoredReasoning =
    shouldStripStoredReasoningForOpenAiResponsesStatelessReplay(options);

  const transformedMessages: AiSdkModelMessage[] = [];
  let pendingToolMediaParts: Extract<AiSdkUserContent, unknown[]> = [];
  const textifyStructuredToolResults = shouldTextifyStructuredToolResults(options);
  const flushPendingToolMedia = () => {
    if (pendingToolMediaParts.length === 0) return;
    transformedMessages.push({ role: "user", content: pendingToolMediaParts });
    pendingToolMediaParts = [];
  };

  for (const message of normalizedMessages) {
    if (message.role !== "tool") {
      flushPendingToolMedia();
    }

    switch (message.role) {
      case "system":
        transformedMessages.push({
          role: "system",
          content: modelMessageContentToText(message.content),
          ...providerOptionsForCacheControl(message.cacheControl),
        });
        break;

      case "user":
        transformedMessages.push({
          role: "user",
          content: toAiSdkUserContent(message.content, options),
          ...providerOptionsForCacheControl(message.cacheControl),
        });
        break;

      case "assistant": {
        const content = toAiSdkAssistantContent(message.content, message.toolCalls, {
          ...options,
          stripOpenAiResponsesStoredReasoning: shouldStripOpenAiResponsesStoredReasoning,
          replayToolCallThoughtSignature: shouldReplayToolCallThoughtSignature(message, options),
        });
        // 模型空回复（如 Speak-only 步骤后 finishReason=stop、text=""）会落成 content ""
        // 的 assistant 记录；Anthropic 把它序列化为空 text block 并以 400
        // "text content blocks must be non-empty" 拒绝，且之后每轮都会回放，会话永久卡死。
        // 只在投影边界丢弃，不改写会话事实，已损坏的会话也能恢复。AI SDK 会合并随后相邻的
        // user 消息；OpenAI 兼容端点（如 DeepSeek）需要交替角色，仍保留空 assistant。
        if (content.length === 0 && isAnthropicWire(options)) break;
        transformedMessages.push({
          role: "assistant",
          content,
          ...providerOptionsForCacheControl(message.cacheControl),
        });
        break;
      }

      case "tool": {
        if (!message.toolCallId || message.toolName === undefined) {
          throw new AiSdkModelAdapterError(
            ModelErrorCode.InvalidModelRequest,
            "Tool model messages require toolCallId and toolName",
            { context: { role: message.role } },
          );
        }
        const toolName = projectToolNameForProvider(message.toolName, options);
        // 含 video 的 tool result 在所有 provider kind 上都强制 textify + 后置投影：
        // AI SDK tool result part 无 video 变体，anthropic 内嵌路径同样会丢失视频内容。
        const messageTextifyToolResult =
          textifyStructuredToolResults || toolResultHasVideoMedia(message.content);

        // 通用 fail-closed：帧引用结果在媒体不可投递时整体错误化（约束：
        // 引用文本不得与"媒体不可用"占位符同现，否则 actionable frame_id 会诱导
        // 模型对未见过画面的坐标产生动作）。
        const frameReferenceFailure = undeliverableFrameReferenceText(message.content, options);

        const toolMediaParts =
          frameReferenceFailure === undefined &&
          messageTextifyToolResult &&
          message.isError !== true
            ? toToolResultMediaUserParts(message.content, {
                ...options,
                toolName,
              })
            : [];
        transformedMessages.push({
          role: "tool",
          content: [
            {
              type: "tool-result",
              toolCallId: message.toolCallId,
              toolName,
              // SDK 合并连续 tool message 时只保留首条的 providerOptions；
              // 缓存标记必须跟随具体结果块，避免批次末尾标记丢失或被移到其他结果。
              ...providerOptionsForCacheControl(message.cacheControl),
              output: frameReferenceFailure
                ? { type: "error-text", value: frameReferenceFailure }
                : toAiSdkToolResultOutput(
                    message.content,
                    options,
                    message.isError === true,
                    messageTextifyToolResult,
                  ),
            },
          ],
        });
        pendingToolMediaParts.push(...toolMediaParts);
        break;
      }
    }
  }

  flushPendingToolMedia();
  return transformedMessages;
}

type AiSdkUserContent = Extract<AiSdkModelMessage, { role: "user" }>["content"];

function toAiSdkToolResultOutput(
  content: ModelMessageContent,
  options: AiSdkMessageTransformOptions,
  isError = false,
  textifyStructuredContent = false,
): AiSdkToolResultOutput {
  if (isError) {
    return { type: "error-text", value: modelMessageContentToText(content) };
  }

  if (typeof content === "string") return { type: "text", value: content };

  const hasToolReferences = content.some((block) => block.type === "tool_reference");
  if (textifyStructuredContent && !hasToolReferences) {
    return { type: "text", value: toStructuredToolResultText(content, options) };
  }

  if (options.stripMedia && !hasToolReferences) {
    return { type: "text", value: modelMessageContentToText(content) };
  }

  // 工具引用不是媒体：媒体降级不能把引用变成名字文本，否则服务端无法展开定义。
  const value = content.flatMap((block) => {
    if (block.type !== "tool_reference" && (textifyStructuredContent || options.stripMedia)) {
      const text = textifyStructuredContent
        ? toStructuredToolResultText([block], options)
        : modelMessageContentToText([block]);
      return text ? [{ type: "text" as const, text }] : [];
    }
    return contentBlockToAiSdkToolResultParts(block, options);
  });
  return value.length > 0
    ? { type: "content", value }
    : { type: "text", value: modelMessageContentToText(content) };
}

function contentBlockToAiSdkToolResultParts(
  block: ModelMessageContentBlock,
  options: AiSdkMessageTransformOptions,
): Extract<AiSdkToolResultOutput, { type: "content" }>["value"] {
  switch (block.type) {
    case "text":
      return block.text.length > 0 ? [{ type: "text", text: block.text }] : [];

    case "tool_reference":
      if (options.providerKind !== "anthropic" || !options.toolSearchEnabled) {
        throw new AiSdkModelAdapterError(
          ModelErrorCode.InvalidModelRequest,
          "Tool references require active Anthropic ToolSearch",
        );
      }
      return [
        {
          type: "custom",
          providerOptions: { anthropic: { type: "tool-reference", toolName: block.toolName } },
        },
      ];

    case "reasoning":
      return [];

    case "image": {
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      const data = dataUrlToDataContent(block.dataUrl);
      if (!data) {
        return [
          { type: "text", text: "ERROR: Image file is empty or corrupted. Inform the user." },
        ];
      }
      const mediaType = providerImageMediaType(block.mediaType, data.data);
      if (!mediaType) return [{ type: "text", text: unsupportedImageMediaTypeText(block) }];
      return [{ type: "image-data", data: data.data, mediaType }];
    }

    case "video": {
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      // AI SDK tool result part 无 video 变体：视频媒体统一由 tool-result-media-projection
      // 拆成后置 user part（toolResultHasVideoMedia 对含 video 的 tool result 在所有
      // provider kind 上强制 textify），这里不产出内嵌 part。
      return [];
    }

    case "file": {
      if (block.text !== undefined && block.text.length > 0) {
        return [{ type: "text", text: block.text }];
      }
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      const data = block.dataUrl ? dataUrlToDataContent(block.dataUrl) : undefined;
      if (data) {
        return [
          {
            type: "file-data",
            data: data.data,
            mediaType: block.mediaType,
            ...(block.name ? { filename: block.name } : {}),
          },
        ];
      }
      return [{ type: "text", text: modelMessageContentToText([block]) }];
    }

    case "resource_link":
      return [{ type: "text", text: modelMessageContentToText([block]) }];
  }
}

function toAiSdkAssistantContent(
  content: ModelMessageContent,
  toolCalls: ModelInputMessage["toolCalls"],
  options: AiSdkAssistantTransformOptions,
): AiSdkAssistantContent {
  // Anthropic 同样拒绝只含空白的 text block。
  if (typeof content === "string" && !toolCalls?.length) {
    return content.trim().length > 0 ? content : "";
  }
  const toToolCallPart = (toolCall: ModelInputToolCall): AiSdkAssistantContentPart => ({
    type: "tool-call",
    toolCallId: toolCall.id,
    toolName: projectToolNameForProvider(toolCall.name, options),
    input: toolCall.input,
    ...(options.replayToolCallThoughtSignature
      ? providerOptionsForToolCallThoughtSignature(toolCall.providerOptions)
      : {}),
  });

  const ordered =
    typeof content === "string" ? undefined : orderedAssistantItems(content, toolCalls);
  if (ordered) {
    // 有序条目（docs/design/v2/model/assistant-block-order.md）：按 orderedAssistantItems 的顺序
    // 输出内容块与工具调用。只改变顺序，不合并块；blockIndex 不进入请求。
    return ordered.flatMap((item) =>
      item.kind === "content"
        ? contentBlockToAiSdkAssistantParts(item.block, options)
        : [toToolCallPart(item.call)],
    );
  }

  const toolCallParts = toolCalls?.map(toToolCallPart) ?? [];
  const contentParts =
    typeof content === "string"
      ? content.trim().length > 0
        ? [{ type: "text" as const, text: content }]
        : []
      : content
          // 修复原因：没有 blockIndex 的旧历史不经规则 a 过滤；Anthropic 拒绝纯空白文字块（400），
          // 旧会话会永久卡死。只在旧条目路径丢弃，有序条目交给规则 a（保留思考块之间的空白）。
          .filter(
            (block) =>
              !(block.type === "text" && block.text.trim().length === 0 && isAnthropicWire(options)),
          )
          .flatMap((block) => contentBlockToAiSdkAssistantParts(block, options));

  return [...contentParts, ...toolCallParts];
}

type ModelInputToolCall = NonNullable<ModelInputMessage["toolCalls"]>[number];

/** 有序条目里的一个块：内容块或工具调用，带它的 blockIndex。 */
export type OrderedAssistantItem =
  | { kind: "content"; block: ModelMessageContentBlock; blockIndex: number }
  | { kind: "tool"; call: ModelInputToolCall; blockIndex: number };

/**
 * 有序条目（docs/design/v2/model/assistant-block-order.md）：内容块与工具调用的 blockIndex 都合法时，
 * 返回按 blockIndex 稳定排序的块，序号相同时内容块在前；否则返回 undefined。
 * 请求排列和 Anthropic 规则（reasoning-history-normalization.ts）都用它，两边的判定和顺序因此一致。
 */
export function orderedAssistantItems(
  content: readonly ModelMessageContentBlock[],
  toolCalls: ModelInputMessage["toolCalls"],
): OrderedAssistantItem[] | undefined {
  const items: OrderedAssistantItem[] = [];
  for (const block of content) {
    const blockIndex = "blockIndex" in block ? block.blockIndex : undefined;
    if (!isBlockIndex(blockIndex)) return undefined;
    items.push({ kind: "content", block, blockIndex });
  }
  for (const call of toolCalls ?? []) {
    if (!isBlockIndex(call.blockIndex)) return undefined;
    items.push({ kind: "tool", call, blockIndex: call.blockIndex });
  }
  return items.sort((left, right) => left.blockIndex - right.blockIndex);
}

function contentBlockToAiSdkAssistantParts(
  block: ModelMessageContentBlock,
  options: AiSdkAssistantTransformOptions,
): AiSdkAssistantContentPart[] {
  switch (block.type) {
    case "tool_reference":
      throw new AiSdkModelAdapterError(
        ModelErrorCode.InvalidModelRequest,
        "Tool references must appear inside tool results",
      );
    case "text":
      // 修复原因：fork 曾在所有线路丢弃纯空白文字块；upstream 的 Anthropic 规则 a
      // （assistant-block-order.md）已过滤空白块，并保留两个思考块之间的空白，否则思考签名前缀被改。
      return block.text.length > 0 ? [{ type: "text", text: block.text }] : [];

    case "reasoning": {
      if (
        options.stripOpenAiResponsesStoredReasoning === true &&
        hasOpenAiStoredReasoningItemId(block.providerOptions)
      ) {
        // 部分 Responses 兼容端点不支持无 previousResponseId 时回放 store=true 的
        // reasoning item_reference；只在 Responses 无状态回放边界丢弃该引用，避免工具结果续轮变成 5xx。
        return [];
      }
      // 没有正文和 provider 元数据的流式 reasoning 空壳会在历史回放时被
      // Anthropic metadata 补全误认为有效 thinking；只在请求投影边界移除精确空壳。
      if (block.text.length === 0 && Object.keys(block.providerOptions ?? {}).length === 0) {
        return [];
      }
      const providerOptions = providerOptionsForReasoningBlock(block, options);

      return [
        {
          type: "reasoning",
          text: block.text,
          ...providerOptions,
        } as AiSdkAssistantContentPart,
      ];
    }

    case "image":
    case "video":
    case "file":
    case "resource_link": {
      const text = modelMessageContentToText([block]);
      return text.length > 0 ? [{ type: "text", text }] : [];
    }
  }
}

function shouldStripStoredReasoningForOpenAiResponsesStatelessReplay(
  options: AiSdkMessageTransformOptions,
): boolean {
  if (options.providerKind !== "openai") return false;
  if (resolveApiFormat(options) !== "openai-responses") return false;
  const openaiOptions = objectRecord(options.providerOptions?.openai);
  if (typeof openaiOptions.previousResponseId === "string") return false;
  if (typeof openaiOptions.conversation === "string") return false;
  if (openaiOptions.store === false) return false;
  return true;
}

function resolveApiFormat(options: AiSdkMessageTransformOptions): string | undefined {
  if (typeof options.apiFormat === "string") return options.apiFormat;
  const apiFormat = options.providerOptions?.apiFormat;
  return typeof apiFormat === "string" ? apiFormat : undefined;
}

function projectToolNameForProvider(
  toolName: unknown,
  options: AiSdkMessageTransformOptions,
): string {
  if (typeof toolName !== "string") {
    throw new AiSdkModelAdapterError(
      ModelErrorCode.InvalidModelRequest,
      "Tool model messages require toolCallId and toolName",
    );
  }
  if (toolName.trim().length > 0) return toolName;

  // OpenAI-compatible wire 不可靠接受空 function name，但历史中的原始
  // 名称仍需保留给 Anthropic 回放；占位值只在 provider 投影边界生成。
  return isAnthropicWire(options) ? toolName : "empty_tool_name";
}

function isAnthropicWire(options: AiSdkMessageTransformOptions): boolean {
  const apiFormat = resolveApiFormat(options);
  if (apiFormat !== undefined) return apiFormat === "anthropic-messages";
  return options.providerKind === "anthropic";
}

/**
 * 修复原因：Gemini（Google OpenAI-compat 端点）要求当前 turn 回放的第一个 function call 带回
 * `extra_content.google.thought_signature`，缺失即 400。`@ai-sdk/openai-compatible` 解析时把签名写到
 * `providerMetadata[<providerId>].thoughtSignature`，序列化时却只读 `providerOptions.google.thoughtSignature`，
 * 所以必须在投影边界转换 key。只对同 provider + 同模型的历史回放，避免把 `extra_content`
 * 发给不认识它的服务；见 model README §3.1.4。
 */
function shouldReplayToolCallThoughtSignature(
  message: ModelInputMessage,
  options: AiSdkMessageTransformOptions,
): boolean {
  if (options.providerKind !== "openai-compatible" || !options.targetModel) return false;
  if (!message.providerId || !message.modelId) return false;
  return (
    message.providerId === options.targetModel.providerId &&
    message.modelId === options.targetModel.modelId
  );
}

function providerOptionsForToolCallThoughtSignature(
  providerOptions: Record<string, unknown> | undefined,
): { providerOptions: AiSdkProviderOptions } | Record<string, never> {
  for (const value of Object.values(providerOptions ?? {})) {
    const thoughtSignature = objectRecord(value).thoughtSignature;
    if (typeof thoughtSignature === "string" && thoughtSignature.length > 0) {
      return { providerOptions: { google: { thoughtSignature } } };
    }
  }
  return {};
}

function hasOpenAiStoredReasoningItemId(providerOptions: unknown): boolean {
  const openaiOptions = objectRecord(objectRecord(providerOptions).openai);
  return typeof openaiOptions.itemId === "string" && openaiOptions.itemId.length > 0;
}

function objectRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

const EMPTY_USER_CONTENT_FALLBACK = "(no content)";

function toAiSdkUserContent(
  content: ModelMessageContent,
  options: AiSdkMessageTransformOptions,
): AiSdkUserContent {
  // 附件-only query 拆出 prompt attachment 后可能留下空 user
  // content，空白占位又可能被 provider trim 后视为缺失 prompt。只在 wire
  // 序列化边界使用固定 fallback，避免改写 session 事实、UI 可见 query 和标题种子。
  if (typeof content === "string") return content || EMPTY_USER_CONTENT_FALLBACK;

  const parts = content.flatMap((block) => contentBlockToAiSdkUserParts(block, options));
  return parts.length > 0 ? parts : EMPTY_USER_CONTENT_FALLBACK;
}

function contentBlockToAiSdkUserParts(
  block: ModelMessageContentBlock,
  options: AiSdkMessageTransformOptions,
): Extract<AiSdkUserContent, unknown[]> {
  switch (block.type) {
    case "tool_reference":
      throw new AiSdkModelAdapterError(
        ModelErrorCode.InvalidModelRequest,
        "Tool references must appear inside tool results",
      );
    case "text":
      return block.text.length > 0 ? [{ type: "text", text: block.text }] : [];

    case "reasoning":
      return block.text.length > 0 ? [{ type: "text", text: block.text }] : [];

    case "image": {
      if (options.stripMedia) {
        return [{ type: "text", text: modelMessageContentToText([block]) }];
      }
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      const data = dataUrlToDataContent(block.dataUrl);
      if (!data) {
        return [
          {
            type: "text",
            text: "ERROR: Image file is empty or corrupted. Inform the user.",
          },
        ];
      }
      const mediaType = providerImageMediaType(block.mediaType, data.data);
      if (!mediaType) return [{ type: "text", text: unsupportedImageMediaTypeText(block) }];
      return [{ type: "image", image: data.data, mediaType }];
    }

    case "video": {
      if (options.stripMedia) {
        return [{ type: "text", text: modelMessageContentToText([block]) }];
      }
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      const data = dataUrlToDataContent(block.dataUrl);
      if (!data) {
        return [
          {
            type: "text",
            text: "ERROR: Video file is empty or corrupted. Inform the user.",
          },
        ];
      }
      // AI SDK 无 video part 类型；mediaType 为自由 string，video/* file part 由
      // patch 后的 @ai-sdk/openai-compatible / @ai-sdk/anthropic 转成 video_url / video block。
      return [{ type: "file", data: data.data, mediaType: block.mediaType }];
    }

    case "file": {
      if (block.text !== undefined && block.text.length > 0) {
        return [{ type: "text", text: block.text }];
      }
      if (options.stripMedia) {
        return [{ type: "text", text: modelMessageContentToText([block]) }];
      }
      const unsupportedText = unsupportedInputMediaText(block, options.inputFormat);
      if (unsupportedText) {
        return [
          {
            type: "text",
            text: unsupportedText,
          },
        ];
      }
      const data = block.dataUrl ? dataUrlToDataContent(block.dataUrl) : undefined;
      if (data) {
        return [
          {
            type: "file",
            data: data.data,
            filename: block.name,
            mediaType: block.mediaType,
          },
        ];
      }
      return [{ type: "text", text: modelMessageContentToText([block]) }];
    }

    case "resource_link":
      return [{ type: "text", text: modelMessageContentToText([block]) }];
  }
}

function providerOptionsForCacheControl(
  cacheControl: ModelCacheControl | undefined,
): { providerOptions: AiSdkProviderOptions } | Record<string, never> {
  if (!cacheControl) {
    return {};
  }

  return {
    providerOptions: {
      anthropic: {
        cacheControl: { ...cacheControl },
      },
    },
  };
}
