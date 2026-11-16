import { describe, expect, it } from "vitest";
import { toAiSdkMessages } from "../src/model/index.js";
import { normalizeReasoningHistory } from "../src/model/reasoning-history-normalization.js";
import { createTestInputFormat } from "./test-model-format.js";

describe("AI SDK model message transform", () => {
  it("orders assistant blocks by blockIndex without merging or leaking the field", () => {
    const [assistant] = toAiSdkMessages([
      {
        role: "assistant",
        content: [
          { type: "reasoning", text: "first thought", blockIndex: 0 },
          { type: "text", text: "A", blockIndex: 1 },
          { type: "reasoning", text: "second thought", blockIndex: 3 },
          { type: "text", text: "B", blockIndex: 4 },
        ],
        toolCalls: [{ id: "t1", name: "Read", input: {}, blockIndex: 2 }],
      },
    ]);
    expect(assistant?.content).toEqual([
      { type: "reasoning", text: "first thought" },
      { type: "text", text: "A" },
      { type: "tool-call", toolCallId: "t1", toolName: "Read", input: {} },
      { type: "reasoning", text: "second thought" },
      { type: "text", text: "B" },
    ]);
    expect(JSON.stringify(assistant)).not.toContain("blockIndex");
  });

  it("keeps the legacy content-then-tools layout when any block lacks blockIndex", () => {
    const [assistant] = toAiSdkMessages([
      {
        role: "assistant",
        content: [{ type: "text", text: "A" }],
        toolCalls: [{ id: "t1", name: "Read", input: {}, blockIndex: 0 }],
      },
    ]);
    expect(assistant?.content).toEqual([
      { type: "text", text: "A" },
      { type: "tool-call", toolCallId: "t1", toolName: "Read", input: {} },
    ]);
  });

  it("uses a fixed wire-only fallback for empty user content", () => {
    expect(
      toAiSdkMessages([
        { role: "user", content: "" },
        { role: "assistant", content: "attachment context acknowledged" },
        { role: "user", content: [] },
        { role: "user", content: [{ type: "text", text: "" }] },
        { role: "user", content: " " },
        { role: "user", content: "\n" },
        { role: "user", content: "  " },
      ]),
    ).toEqual([
      { role: "user", content: "(no content)" },
      { role: "assistant", content: "attachment context acknowledged" },
      { role: "user", content: "(no content)" },
      { role: "user", content: "(no content)" },
      { role: "user", content: " " },
      { role: "user", content: "\n" },
      { role: "user", content: "  " },
    ]);
  });

  it("drops assistant turns that have nothing to send on the Anthropic wire", () => {
    // A turn that ended with an empty model reply (e.g. after a Speak-only step) is
    // stored as content "" and must not be replayed as an empty text block.
    const history = [
      { role: "user" as const, content: "write the doc" },
      { role: "assistant" as const, content: "" },
      { role: "user" as const, content: "where is it?" },
      { role: "assistant" as const, content: " \n" },
      { role: "assistant" as const, content: [{ type: "text" as const, text: "" }] },
      { role: "user" as const, content: "still there?" },
      {
        role: "assistant" as const,
        content: [
          { type: "text" as const, text: "  " },
          { type: "text" as const, text: "yes" },
        ],
      },
    ];
    for (const options of [
      { providerKind: "anthropic" as const },
      { providerKind: "custom" as const, apiFormat: "anthropic-messages" },
    ]) {
      // Legacy entries (no blockIndex) skip rule a, so this layer drops their whitespace blocks.
      expect(toAiSdkMessages(history, options)).toEqual([
        { role: "user", content: "write the doc" },
        { role: "user", content: "where is it?" },
        { role: "user", content: "still there?" },
        { role: "assistant", content: [{ type: "text", text: "yes" }] },
      ]);
    }
    // OpenAI-compatible endpoints may require alternating roles, so the empty turn stays.
    expect(toAiSdkMessages(history, { providerKind: "openai-compatible" })).toEqual([
      { role: "user", content: "write the doc" },
      { role: "assistant", content: "" },
      { role: "user", content: "where is it?" },
      { role: "assistant", content: "" },
      { role: "assistant", content: [] },
      { role: "user", content: "still there?" },
      {
        role: "assistant",
        content: [
          { type: "text", text: "  " },
          { type: "text", text: "yes" },
        ],
      },
    ]);
  });

  it("drops a whitespace text block beside visible text once Anthropic normalization ran", () => {
    // The real Anthropic request path (runner projectRequestHistory) normalizes before
    // serializing; rule a removes whitespace text unless it sits between two thinking blocks.
    const normalized = normalizeReasoningHistory([
      { role: "user", content: "still there?" },
      {
        role: "assistant",
        content: [
          { type: "text", text: "  ", blockIndex: 0 },
          { type: "text", text: "yes", blockIndex: 1 },
        ],
      },
      { role: "user", content: "ok" },
    ]);
    expect(toAiSdkMessages(normalized, { providerKind: "anthropic" })).toEqual([
      { role: "user", content: "still there?" },
      { role: "assistant", content: [{ type: "text", text: "yes" }] },
      { role: "user", content: "ok" },
    ]);
  });

  it("keeps ordered whitespace between two thinking blocks on the Anthropic wire", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "a", blockIndex: 0 },
            { type: "text", text: "\n\n", blockIndex: 1 },
            { type: "reasoning", text: "b", blockIndex: 2 },
            { type: "text", text: "done", blockIndex: 3 },
          ],
        },
      ],
      { providerKind: "anthropic" },
    );
    const parts = messages[0]?.content as Array<{ type: string; text?: string }>;
    expect(parts.map((part) => part.text)).toEqual(["a", "\n\n", "b", "done"]);
  });

  it("keeps an empty-text assistant turn that still carries tool calls", () => {
    expect(
      toAiSdkMessages([
        { role: "assistant", content: "", toolCalls: [{ id: "t1", name: "Speak", input: {} }] },
      ]),
    ).toEqual([
      {
        role: "assistant",
        content: [{ type: "tool-call", toolCallId: "t1", toolName: "Speak", input: {} }],
      },
    ]);
  });

  it("never declares an image media type the provider rejects", () => {
    const image = (mediaType: string, base64: string) => ({
      type: "image" as const,
      mediaType,
      dataUrl: `data:${mediaType};base64,${base64}`,
    });
    const [message] = toAiSdkMessages([
      {
        role: "user",
        content: [
          // Windows clipboard DIB pasted from GPUI ("BM" magic).
          image("image/bmp", "Qk02AAAAAAAAADYAAAAoAAAA"),
          image("image/jpg", "aW1hZ2U="),
          image("image/bmp", "iVBORw0KGgoAAAANSUhEUg=="),
        ],
      },
    ]);
    expect(message?.content).toEqual([
      {
        type: "text",
        text: expect.stringContaining(
          "Image omitted from provider request: image/bmp is not a supported image format",
        ),
      },
      { type: "image", image: "aW1hZ2U=", mediaType: "image/jpeg" },
      { type: "image", image: "iVBORw0KGgoAAAANSUhEUg==", mediaType: "image/png" },
    ]);
  });

  it("does not append the empty-user fallback to media or resource-only content", () => {
    const messages = toAiSdkMessages([
      {
        role: "user",
        content: [
          {
            type: "image",
            mediaType: "image/png",
            dataUrl: "data:image/png;base64,aW1hZ2U=",
          },
        ],
      },
      {
        role: "user",
        content: [
          {
            type: "file",
            mediaType: "application/pdf",
            name: "report.pdf",
            dataUrl: "data:application/pdf;base64,cGRm",
          },
        ],
      },
      {
        role: "user",
        content: [{ type: "resource_link", uri: "https://example.com/ref" }],
      },
    ]);

    expect(messages.map((message) => message.content)).toEqual([
      [{ type: "image", image: "aW1hZ2U=", mediaType: "image/png" }],
      [
        {
          type: "file",
          data: "cGRm",
          filename: "report.pdf",
          mediaType: "application/pdf",
        },
      ],
      [{ type: "text", text: "[Resource: https://example.com/ref]" }],
    ]);
    expect(JSON.stringify(messages)).not.toContain("(no content)");
  });

  it("concatenates self-delimited leading system messages for OpenAI-compatible providers", () => {
    const messages = toAiSdkMessages(
      [
        { role: "system", content: "cli prefix" },
        {
          role: "system",
          content: [{ type: "text", text: "\nstable context" }],
          cacheControl: { type: "ephemeral" },
        },
        {
          role: "system",
          content: "\n\ndynamic context\n",
          cacheControl: { type: "ephemeral", ttl: "1h" },
        },
        { role: "user", content: "first question" },
        { role: "assistant", content: "first answer" },
        { role: "user", content: "follow-up" },
      ],
      { providerKind: "openai-compatible" },
    );

    expect(messages).toEqual([
      {
        role: "system",
        content: "cli prefix\nstable context\n\ndynamic context\n",
        providerOptions: {
          anthropic: {
            cacheControl: { type: "ephemeral", ttl: "1h" },
          },
        },
      },
      { role: "user", content: "first question" },
      { role: "assistant", content: "first answer" },
      { role: "user", content: "follow-up" },
    ]);
  });

  it("keeps zero or one leading system message unchanged for OpenAI-compatible providers", () => {
    expect(
      toAiSdkMessages([{ role: "user", content: "question" }], {
        providerKind: "openai-compatible",
      }),
    ).toEqual([{ role: "user", content: "question" }]);

    expect(
      toAiSdkMessages(
        [
          { role: "system", content: "single system" },
          { role: "user", content: "question" },
        ],
        { providerKind: "openai-compatible" },
      ),
    ).toEqual([
      { role: "system", content: "single system" },
      { role: "user", content: "question" },
    ]);
  });

  it("preserves non-leading system messages for OpenAI-compatible providers", () => {
    const messages = toAiSdkMessages(
      [
        { role: "system", content: "first leading system" },
        { role: "system", content: "second leading system" },
        { role: "user", content: "question" },
        { role: "system", content: "mid-conversation system" },
        { role: "assistant", content: "answer" },
      ],
      { providerKind: "openai-compatible" },
    );

    expect(messages).toEqual([
      {
        role: "system",
        content: "first leading systemsecond leading system",
      },
      { role: "user", content: "question" },
      { role: "system", content: "mid-conversation system" },
      { role: "assistant", content: "answer" },
    ]);
  });

  it("preserves multiple system messages for non-OpenAI-compatible providers", () => {
    const messages = toAiSdkMessages(
      [
        { role: "system", content: "first system" },
        { role: "system", content: "second system" },
        { role: "user", content: "question" },
      ],
      { providerKind: "anthropic" },
    );

    expect(messages).toEqual([
      { role: "system", content: "first system" },
      { role: "system", content: "second system" },
      { role: "user", content: "question" },
    ]);
  });

  it("preserves provider tool call ids across assistant calls and tool results", () => {
    const toolCallId = "call_-7666958008559069619";
    const messages = toAiSdkMessages([
      {
        role: "assistant",
        content: "",
        toolCalls: [{ id: toolCallId, name: "Read", input: { file_path: "README.md" } }],
      },
      {
        role: "tool",
        content: "ok",
        toolCallId,
        toolName: "Read",
      },
    ]);

    const assistantToolCall = (messages[0] as any).content[0];
    const toolResult = (messages[1] as any).content[0];

    expect(assistantToolCall.toolCallId).toBe(toolCallId);
    expect(toolResult.toolCallId).toBe(toolCallId);
  });

  it("drops OpenAI Responses stored reasoning item references on stateless replay", () => {
    const toolCallId = "call_read";
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            {
              type: "reasoning",
              text: "stored reasoning",
              providerOptions: { openai: { itemId: "rs_1" } },
            },
            { type: "text", text: "checking" },
          ],
          toolCalls: [{ id: toolCallId, name: "Read", input: { file_path: "README.md" } }],
        },
      ],
      {
        providerKind: "openai",
        providerOptions: { apiFormat: "openai-responses" },
      },
    );

    expect((messages[0] as any).content).toEqual([
      { type: "text", text: "checking" },
      {
        type: "tool-call",
        toolCallId,
        toolName: "Read",
        input: { file_path: "README.md" },
      },
    ]);
  });

  it("preserves OpenAI Responses stored reasoning item references with previousResponseId", () => {
    const toolCallId = "call_read";
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            {
              type: "reasoning",
              text: "stored reasoning",
              providerOptions: { openai: { itemId: "rs_1" } },
            },
            { type: "text", text: "checking" },
          ],
          toolCalls: [{ id: toolCallId, name: "Read", input: { file_path: "README.md" } }],
        },
      ],
      {
        providerKind: "openai",
        providerOptions: {
          apiFormat: "openai-responses",
          openai: { previousResponseId: "resp_1" },
        },
      },
    );

    expect((messages[0] as any).content).toEqual([
      {
        type: "reasoning",
        text: "stored reasoning",
        providerOptions: { openai: { itemId: "rs_1" } },
      },
      { type: "text", text: "checking" },
      {
        type: "tool-call",
        toolCallId,
        toolName: "Read",
        input: { file_path: "README.md" },
      },
    ]);
  });

  it("does not drop stored reasoning references for OpenAI-compatible providers", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            {
              type: "reasoning",
              text: "compatible reasoning",
              providerOptions: { openai: { itemId: "rs_1" } },
            },
            { type: "text", text: "done" },
          ],
        },
      ],
      {
        providerKind: "openai-compatible",
        providerOptions: { apiFormat: "openai-responses" },
      },
    );

    expect((messages[0] as any).content).toEqual([
      {
        type: "reasoning",
        text: "compatible reasoning",
        providerOptions: { openai: { itemId: "rs_1" } },
      },
      { type: "text", text: "done" },
    ]);
  });

  it("translates image tool results into structured media output", () => {
    const toolCallId = "call_read_image";
    const messages = toAiSdkMessages([
      {
        role: "tool",
        content: [
          {
            type: "image",
            mediaType: "image/jpeg",
            dataUrl: "data:image/jpeg;base64,cmVhZC1pbWFnZQ==",
            source: { id: "read-image", kind: "inline", placeholder: "Read image" },
          },
        ],
        toolCallId,
        toolName: "Read",
      },
    ]);

    expect((messages[0] as any).content[0]).toEqual({
      type: "tool-result",
      toolCallId,
      toolName: "Read",
      output: {
        type: "content",
        value: [
          {
            type: "image-data",
            data: "cmVhZC1pbWFnZQ==",
            mediaType: "image/jpeg",
          },
        ],
      },
    });
  });

  it("does not send empty reasoning_content for a plain assistant history", () => {
    const messages = toAiSdkMessages([{ role: "assistant", content: "done" }], {});

    expect(messages).toEqual([{ role: "assistant", content: "done" }]);
  });

  it("removes an empty reasoning block without configured replay metadata", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "" },
            { type: "text", text: "done" },
          ],
        },
      ],
      {},
    );

    expect(messages).toEqual([{ role: "assistant", content: [{ type: "text", text: "done" }] }]);
  });

  it("removes a sole empty reasoning block without configured replay metadata", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [{ type: "reasoning", text: "" }],
        },
      ],
      {},
    );

    expect(messages).toEqual([{ role: "assistant", content: [] }]);
  });

  it("preserves signed reasoning blocks on Anthropic-compatible transport", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            {
              type: "reasoning",
              text: "anthropic thinking",
              providerOptions: { anthropic: { signature: "sig_1" } },
            },
            { type: "text", text: "done" },
          ],
        },
      ],
      {
        providerKind: "anthropic",
      },
    );

    expect((messages[0] as any).providerOptions).toBeUndefined();
    expect((messages[0] as any).content).toEqual([
      {
        type: "reasoning",
        text: "anthropic thinking",
        providerOptions: { anthropic: { signature: "sig_1" } },
      },
      { type: "text", text: "done" },
    ]);
  });

  it.each([
    ["missing provider metadata", undefined],
    ["empty provider metadata", {}],
  ] as const)("removes exact-empty reasoning stubs with %s", (_name, providerOptions) => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "", providerOptions },
            { type: "text", text: "done" },
          ],
        },
      ],
      { providerKind: "anthropic" },
    );

    expect((messages[0] as any).content).toEqual([{ type: "text", text: "done" }]);
  });

  it("preserves whitespace-only reasoning without provider metadata", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: " \n" },
            { type: "text", text: "done" },
          ],
        },
      ],
      { providerKind: "anthropic" },
    );

    expect((messages[0] as any).content).toEqual([
      {
        type: "reasoning",
        text: " \n",
        providerOptions: { anthropic: { signature: "" } },
      },
      { type: "text", text: "done" },
    ]);
  });

  it.each([
    [
      "an empty Anthropic signature",
      { anthropic: { signature: "" } },
      { providerKind: "anthropic" },
    ],
    [
      "Anthropic redacted data",
      { anthropic: { redactedData: "opaque" } },
      { providerKind: "anthropic" },
    ],
    [
      "an OpenAI Responses item id",
      { openai: { itemId: "rs_empty" } },
      {
        providerKind: "openai",
        providerOptions: {
          apiFormat: "openai-responses",
          openai: { previousResponseId: "resp_1" },
        },
      },
    ],
    [
      "OpenAI Responses encrypted content",
      { openai: { reasoningEncryptedContent: "encrypted" } },
      {
        providerKind: "openai",
        providerOptions: { apiFormat: "openai-responses", openai: { store: false } },
      },
    ],
    [
      "unknown provider metadata",
      { futureProvider: { opaque: "value" } },
      { providerKind: "custom" },
    ],
  ] as const)("preserves exact-empty reasoning carrying %s", (_name, providerOptions, options) => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "", providerOptions },
            { type: "text", text: "done" },
          ],
        },
      ],
      options,
    );

    expect((messages[0] as any).content[0]).toEqual({
      type: "reasoning",
      text: "",
      providerOptions,
    });
  });

  it("adds empty Anthropic signature for unsigned reasoning blocks", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "thinking before a tool" },
            { type: "text", text: "done" },
          ],
        },
      ],
      {
        providerKind: "anthropic",
      },
    );

    expect((messages[0] as any).content).toEqual([
      {
        type: "reasoning",
        text: "thinking before a tool",
        providerOptions: { anthropic: { signature: "" } },
      },
      { type: "text", text: "done" },
    ]);
  });

  it("does not apply reasoning replay without explicit compatibility", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "legacy deepseek reasoning" },
            { type: "text", text: "done" },
          ],
        },
      ],
      {
        providerKind: "openai-compatible",
      },
    );

    expect((messages[0] as any).providerOptions).toBeUndefined();
    expect((messages[0] as any).content).toEqual([
      { type: "reasoning", text: "legacy deepseek reasoning" },
      { type: "text", text: "done" },
    ]);
  });

  it("preserves reasoning blocks for assistant history without replay compatibility", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "assistant",
          content: [
            { type: "reasoning", text: "private scratchpad" },
            { type: "text", text: "done" },
          ],
        },
      ],
      {},
    );

    expect((messages[0] as any).content).toEqual([
      { type: "reasoning", text: "private scratchpad" },
      { type: "text", text: "done" },
    ]);
  });

  it("translates provider-neutral cache control to Anthropic provider options", () => {
    const messages = toAiSdkMessages([
      {
        role: "system",
        content: "stable context",
        cacheControl: { type: "ephemeral" },
      },
      {
        role: "user",
        content: "prompt",
        cacheControl: { type: "ephemeral", ttl: "1h" },
      },
      {
        role: "tool",
        content: "tool result",
        toolCallId: "tool-1",
        toolName: "Read",
        cacheControl: { type: "ephemeral" },
      },
    ]);

    expect((messages[0] as any).providerOptions).toEqual({
      anthropic: {
        cacheControl: { type: "ephemeral" },
      },
    });
    expect((messages[1] as any).providerOptions).toEqual({
      anthropic: {
        cacheControl: { type: "ephemeral", ttl: "1h" },
      },
    });
    expect((messages[2] as any).providerOptions).toBeUndefined();
    expect((messages[2] as any).content[0].providerOptions).toEqual({
      anthropic: {
        cacheControl: { type: "ephemeral" },
      },
    });
  });

  it("translates user image content blocks to AI SDK image parts", () => {
    const messages = toAiSdkMessages([
      {
        role: "user",
        content: [
          { type: "text", text: "what is in this image?" },
          {
            type: "image",
            mediaType: "image/png",
            dataUrl: "data:image/png;base64,aW1hZ2U=",
            source: { id: "image-1", kind: "inline", placeholder: "[Image 1]" },
          },
        ],
      },
    ]);

    expect((messages[0] as any).content).toEqual([
      { type: "text", text: "what is in this image?" },
      { type: "image", image: "aW1hZ2U=", mediaType: "image/png" },
    ]);
  });

  it("can strip media blocks to stable text placeholders", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "user",
          content: [
            {
              type: "image",
              mediaType: "image/png",
              dataUrl: "data:image/png;base64,aW1hZ2U=",
              source: { id: "image-1", kind: "inline", placeholder: "[Image 1]" },
            },
          ],
        },
      ],
      { stripMedia: true },
    );

    expect((messages[0] as any).content).toEqual([
      { type: "text", text: "[Attached image/png: [Image 1]]" },
    ]);
  });

  it("turns unsupported image input into an explicit model-visible error", () => {
    const messages = toAiSdkMessages(
      [
        {
          role: "user",
          content: [
            {
              type: "image",
              mediaType: "image/png",
              dataUrl: "data:image/png;base64,aW1hZ2U=",
            },
          ],
        },
      ],
      { inputFormat: createTestInputFormat({ supportsImage: false }) },
    );

    expect((messages[0] as any).content).toEqual([
      {
        type: "text",
        text: "[Attached image/png]\n[Media omitted from provider request because the selected model does not support image input.]",
      },
    ]);
  });
});
