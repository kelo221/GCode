import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createProjectId, createSessionId } from "@zcode/contracts";
import type { FileSystemPort, ModelEvent, ModelRequest } from "@zcode/contracts";
import { describe, expect, it } from "vitest";
import { createSqliteSessionStore } from "../../adapters/src/storage/session-store/sqlite-session-store.js";
import { createTestAgentRuntime } from "./test-agent-runtime.js";
import { createTestSessionEventStore } from "./test-event-store.js";
import { createTestModelSelection } from "./test-model-selection.js";
import { createTestModelFactory } from "./test-runtime-model.js";

const CONTENT_CHUNKS = 400;

/** 只有 stat 的文件系统：existing.txt 存在，其余不存在。截断判定只用到 stat。 */
function statOnlyFileSystem(statCalls: string[]): FileSystemPort {
  return {
    async stat({ path }: { path: string }) {
      statCalls.push(path);
      const exists = path.replaceAll("\\", "/").endsWith("/existing.txt");
      return { path, kind: exists ? "file" : "missing", sizeBytes: exists ? 10 : 0 };
    },
  } as unknown as FileSystemPort;
}

/** 先写一句话，再开始一个 Write：file_path 之后无穷地流 content，直到被 abort 或耗尽。 */
async function* writeStream(
  request: ModelRequest,
  filePath: string,
  progress: { contentChunks: number; aborted: boolean },
  openThinking: boolean,
): AsyncIterable<ModelEvent> {
  if (openThinking) {
    // 人为把 thinking 拖到 Write 参数之后才结束，验证截断会等签名到达。
    yield { id: "r1", type: "reasoning_start" };
    yield { id: "r1", type: "reasoning_delta", text: "Plan the rewrite." };
  }
  yield { id: "t1", type: "text_start" };
  yield { id: "t1", type: "text_delta", text: "Rewriting the file." };
  yield { id: "t1", type: "text_end" };
  yield { id: "w1", type: "tool_input_start", toolName: "Write" };
  yield { id: "w1", type: "tool_input_delta", delta: `{"file_path":${JSON.stringify(filePath)},` };
  yield { id: "w1", type: "tool_input_delta", delta: `"content":"` };
  for (let i = 0; i < CONTENT_CHUNKS; i += 1) {
    if (request.abortSignal?.aborted) {
      progress.aborted = true;
      return;
    }
    if (openThinking && i === 20) {
      yield {
        id: "r1",
        type: "reasoning_end",
        providerMetadata: { anthropic: { signature: "sig" } },
      };
    }
    progress.contentChunks += 1;
    yield { id: "w1", type: "tool_input_delta", delta: "line of generated content\\n" };
    // setImmediate 而不是 setTimeout：Windows 计时器粒度约 15ms，400 次会超时。
    await new Promise((resolve) => setImmediate(resolve));
  }
  yield { id: "w1", type: "tool_input_delta", delta: `"}` };
  yield { id: "w1", type: "tool_input_end" };
  yield {
    type: "tool_call",
    toolCall: { id: "w1", name: "Write", input: { file_path: filePath, content: "x" } },
  };
  yield { type: "finish", finishReason: "tool-calls", usage: {} };
}

async function* doneStream(): AsyncIterable<ModelEvent> {
  yield { id: "t2", type: "text_start" };
  yield { id: "t2", type: "text_delta", text: "Done." };
  yield { id: "t2", type: "text_end" };
  yield { type: "finish", finishReason: "stop", usage: {} };
}

async function runTurn(filePath: string, modelSelection?: string, openThinking = false) {
  const root = await mkdtemp(join(tmpdir(), "write-cutoff-"));
  const sessionId = createSessionId(`write-cutoff-${filePath.length}`);
  const store = await createSqliteSessionStore({ dbPath: join(root, "s.sqlite") });
  await store.createSession({
    id: sessionId,
    projectID: createProjectId("write-cutoff"),
    slug: "write-cutoff",
    title: "Write cutoff",
    directory: root,
    version: "test",
    time: { created: 1, updated: 1 },
  });
  const requests: ModelRequest[] = [];
  const statCalls: string[] = [];
  const progress = { contentChunks: 0, aborted: false };
  const runtime = createTestAgentRuntime(
    sessionId,
    {
      mode: "yolo",
      ...(modelSelection ? { modelSelection: createTestModelSelection(modelSelection) } : {}),
      modelStreaming: "on",
      titleGeneration: { enabled: false },
      workingDirectory: root,
      workspacePath: root,
    },
    {
      sessionStore: store,
      eventStore: createTestSessionEventStore(),
      fileSystemPort: statOnlyFileSystem(statCalls),
      modelFactory: createTestModelFactory({
        streamText(request) {
          requests.push(request);
          return requests.length === 1
            ? writeStream(request, filePath, progress, openThinking)
            : doneStream();
        },
      }),
    },
  );
  try {
    await runtime.executeTurn("Rewrite the file.");
    const parts = (await store.messages({ sessionID: sessionId })).flatMap((m) => m.parts);
    return { requests, statCalls, progress, parts };
  } finally {
    await runtime.dispose?.();
    store.close?.();
    await rm(root, { recursive: true, force: true });
  }
}

describe("unread Write stream cutoff", () => {
  it("stops the response once file_path names an existing unread file", async () => {
    const { requests, statCalls, progress, parts } = await runTurn("existing.txt");

    expect(statCalls).toHaveLength(1);
    // 截断时 runtime 先 abort 这次请求（真实 adapter 靠它断开连接），再关闭流迭代器。
    expect(requests[0]?.abortSignal?.aborted).toBe(true);
    expect(progress.contentChunks).toBeLessThan(CONTENT_CHUNKS);
    expect(requests).toHaveLength(2);
    expect(requests[1]?.abortSignal?.aborted).toBe(false);

    const writePart = parts.find((p) => p.type === "tool" && p.tool === "Write");
    expect(writePart).toMatchObject({
      state: { status: "error", input: { file_path: "existing.txt" } },
    });
    expect(JSON.stringify(writePart)).toContain("File has not been read yet.");
    // 截断前的回复文本保留。
    expect(parts.some((p) => p.type === "text" && p.text === "Rewriting the file.")).toBe(true);
    // 下一次请求带上了被截 Write 的结果。
    expect(JSON.stringify(requests[1]?.messages)).toContain("none of its content was used");
  });

  it("never cuts Gemini, whose thought signature arrives with the full tool call", async () => {
    const { progress, statCalls } = await runTurn("existing.txt", "proxy/gemini-3-pro");
    expect(statCalls).toHaveLength(0);
    expect(progress.contentChunks).toBe(CONTENT_CHUNKS);
  });

  it("waits for an open thinking block to end before cutting", async () => {
    const { requests, parts, progress } = await runTurn("existing.txt", undefined, true);
    expect(requests[0]?.abortSignal?.aborted).toBe(true);
    expect(progress.contentChunks).toBeGreaterThanOrEqual(20);
    expect(progress.contentChunks).toBeLessThan(CONTENT_CHUNKS);
    const reasoning = parts.find((p) => p.type === "reasoning");
    expect(reasoning).toMatchObject({ metadata: { anthropic: { signature: "sig" } } });
  });

  it("lets a Write to a new file stream to the end", async () => {
    const { progress, requests, parts } = await runTurn("new-file.txt");
    expect(progress.aborted).toBe(false);
    expect(requests[0]?.abortSignal?.aborted).toBe(false);
    expect(progress.contentChunks).toBe(CONTENT_CHUNKS);
    expect(requests).toHaveLength(2);
    const writePart = parts.find((p) => p.type === "tool" && p.tool === "Write");
    expect(JSON.stringify(writePart)).not.toContain("none of its content was used");
  });
});
