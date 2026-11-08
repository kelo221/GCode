type ProviderFetch = typeof globalThis.fetch;

const SSE_FRAME_SEPARATOR_PATTERN = /\r\n\r\n|\n\n|\r\r/;
// 只有疑似数组 content 的帧才解析；字符串 content 的帧不经 JSON 往返，逐字节透传。
const CONTENT_ARRAY_PATTERN = /"content"\s*:\s*\[/;
const REASONING_CONTENT_PATTERN = /"reasoning_content"\s*:/;

/**
 * 修复原因：Mistral 的 zai-glm-5-3 / zai-glm-5-3-flash 总是把 `content` 返回为 chunk 列表
 * （`thinking` + `text`），而 `@ai-sdk/openai-compatible` 的 chunk / response schema 只接受字符串，
 * 整帧以 AI_TypeValidationError 失败。这里在 SDK 解析前把数组拆回 SDK 已支持的字符串字段：
 * text chunk 进入 `content`，thinking chunk 进入 `reasoning_content`，因此思考不会混入可见回复。
 * 字符串 content 的帧和响应原样透传（docs/design/v2/model/README.md §3.1.3）。
 *
 * 请求侧：SDK 把历史 assistant 的 reasoning 编码为 `reasoning_content` 回放，Mistral 的 schema
 * 以 422 `extra_forbidden` 拒绝该字段，导致带思考的首轮之后每一轮都失败。只对 Mistral 端点删除它；
 * DeepSeek 等其它服务需要回放 reasoning_content，保持原样。
 */
export function createOpenAICompatibleContentPartsCompatFetch(
  baseFetch: ProviderFetch,
): ProviderFetch {
  return async (input, init) => {
    const response = await baseFetch(input, withoutMistralReasoningHistory(input, init));
    if (!response.ok) {
      return response;
    }
    if (isEventStream(response)) {
      return rewriteEventStream(response);
    }
    return isJson(response) ? rewriteJsonResponse(response) : response;
  };
}

/** 返回归一化后的 chunk / response；没有数组 content 时返回 undefined，调用方保留原值。 */
export function normalizeOpenAICompatibleContentParts(
  value: unknown,
): Record<string, unknown> | undefined {
  const body = asRecord(value);
  if (!body || !Array.isArray(body.choices)) {
    return undefined;
  }

  let changed = false;
  const choices = body.choices.map((item) => {
    const choice = asRecord(item);
    if (!choice) {
      return item;
    }
    const delta = normalizeContentHolder(choice.delta);
    const message = normalizeContentHolder(choice.message);
    if (!delta && !message) {
      return item;
    }
    changed = true;
    return { ...choice, ...(delta ? { delta } : {}), ...(message ? { message } : {}) };
  });

  return changed ? { ...body, choices } : undefined;
}

/** 首个有效输出计时用：数组 content 中有非空 text 或 thinking 即视为内容。 */
export function hasNonEmptyContentParts(value: unknown): boolean {
  if (!Array.isArray(value)) {
    return false;
  }
  const { text, reasoning } = splitContentParts(value);
  return text.length > 0 || reasoning.length > 0;
}

/** 删除发往 Mistral 的历史 assistant `reasoning_content`；无需改写时返回原 init。 */
function withoutMistralReasoningHistory(
  input: Parameters<ProviderFetch>[0],
  init: RequestInit | undefined,
): RequestInit | undefined {
  if (
    typeof init?.body !== "string" ||
    !REASONING_CONTENT_PATTERN.test(init.body) ||
    !isMistralEndpoint(input)
  ) {
    return init;
  }
  const body = asRecord(safeParse(init.body));
  if (!body || !Array.isArray(body.messages)) {
    return init;
  }

  let changed = false;
  const messages = body.messages.map((item) => {
    const message = asRecord(item);
    if (message?.role !== "assistant" || !("reasoning_content" in message)) {
      return item;
    }
    changed = true;
    const stripped = { ...message };
    delete stripped.reasoning_content;
    return stripped;
  });

  return changed ? { ...init, body: JSON.stringify({ ...body, messages }) } : init;
}

function isMistralEndpoint(input: Parameters<ProviderFetch>[0]): boolean {
  const href = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  try {
    const hostname = new URL(href).hostname.toLowerCase();
    return hostname === "mistral.ai" || hostname.endsWith(".mistral.ai");
  } catch {
    return false;
  }
}

function normalizeContentHolder(value: unknown): Record<string, unknown> | undefined {
  const holder = asRecord(value);
  if (!holder || !Array.isArray(holder.content)) {
    return undefined;
  }

  const { text, reasoning } = splitContentParts(holder.content);
  const normalized: Record<string, unknown> = { ...holder, content: text };
  if (reasoning.length > 0) {
    // SDK 按 `reasoning_content ?? reasoning` 读取；追加到已有值之后，避免覆盖同帧的字符串 reasoning。
    normalized.reasoning_content = `${existingReasoning(holder)}${reasoning}`;
  }
  return normalized;
}

function splitContentParts(parts: readonly unknown[]): { text: string; reasoning: string } {
  let text = "";
  let reasoning = "";
  for (const item of parts) {
    const part = asRecord(item);
    if (part?.type === "text" && typeof part.text === "string") {
      text += part.text;
    } else if (part?.type === "thinking") {
      reasoning += thinkingText(part.thinking);
    }
  }
  return { text, reasoning };
}

function thinkingText(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (!Array.isArray(value)) {
    return "";
  }
  let text = "";
  for (const item of value) {
    const part = asRecord(item);
    if (typeof part?.text === "string") {
      text += part.text;
    }
  }
  return text;
}

function existingReasoning(holder: Record<string, unknown>): string {
  if (typeof holder.reasoning_content === "string") {
    return holder.reasoning_content;
  }
  return typeof holder.reasoning === "string" ? holder.reasoning : "";
}

function rewriteEventStream(response: Response): Response {
  const body = response.body;
  if (!body) {
    return response;
  }

  return new Response(body.pipeThrough(createContentPartsStreamTransform()), {
    headers: withoutRepresentationHeaders(response.headers),
    status: response.status,
    statusText: response.statusText,
  });
}

async function rewriteJsonResponse(response: Response): Promise<Response> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(await response.clone().text());
  } catch {
    return response;
  }

  const normalized = normalizeOpenAICompatibleContentParts(parsed);
  if (!normalized) {
    return response;
  }

  return new Response(JSON.stringify(normalized), {
    headers: withoutRepresentationHeaders(response.headers),
    status: response.status,
    statusText: response.statusText,
  });
}

function createContentPartsStreamTransform(): TransformStream<Uint8Array, Uint8Array> {
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  let pending = "";

  return new TransformStream<Uint8Array, Uint8Array>({
    transform(chunk, controller) {
      pending += decoder.decode(chunk, { stream: true });
      pending = emitCompleteFrames(pending, controller, encoder);
    },
    flush(controller) {
      pending += decoder.decode();
      pending = emitCompleteFrames(pending, controller, encoder);
      if (pending.length > 0) {
        controller.enqueue(encoder.encode(rewriteFrame(pending)));
      }
    },
  });
}

function emitCompleteFrames(
  input: string,
  controller: TransformStreamDefaultController<Uint8Array>,
  encoder: TextEncoder,
): string {
  let pending = input;
  for (;;) {
    const match = SSE_FRAME_SEPARATOR_PATTERN.exec(pending);
    if (!match) {
      return pending;
    }

    const separator = match[0] ?? "";
    const frame = pending.slice(0, match.index);
    pending = pending.slice(match.index + separator.length);
    controller.enqueue(encoder.encode(`${rewriteFrame(frame)}${separator}`));
  }
}

function rewriteFrame(frame: string): string {
  if (!CONTENT_ARRAY_PATTERN.test(frame)) {
    return frame;
  }

  const lines = frame.split(/\r\n|\n|\r/);
  const dataLines = lines.filter((line) => line.startsWith("data:"));
  if (dataLines.length === 0) {
    return frame;
  }
  const data = dataLines.map((line) => line.slice("data:".length).replace(/^ /, "")).join("\n");
  const normalized = normalizeOpenAICompatibleContentParts(safeParse(data));
  if (!normalized) {
    return frame;
  }

  const lineEnding = frame.includes("\r\n") ? "\r\n" : frame.includes("\r") ? "\r" : "\n";
  const output: string[] = [];
  let wroteData = false;
  for (const line of lines) {
    if (!line.startsWith("data:")) {
      output.push(line);
    } else if (!wroteData) {
      output.push(`data: ${JSON.stringify(normalized)}`);
      wroteData = true;
    }
  }
  return output.join(lineEnding);
}

function isEventStream(response: Response): boolean {
  return response.headers.get("content-type")?.toLowerCase().includes("event-stream") === true;
}

function isJson(response: Response): boolean {
  const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
  return contentType.includes("application/json") || contentType.includes("+json");
}

function withoutRepresentationHeaders(source: Headers): Headers {
  const headers = new Headers(source);
  headers.delete("content-length");
  headers.delete("content-encoding");
  return headers;
}

function safeParse(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return undefined;
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  return value as Record<string, unknown>;
}
