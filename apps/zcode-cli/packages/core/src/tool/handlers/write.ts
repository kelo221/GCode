// ============================================================
// Write Tool Handler
// ============================================================

import type { ToolHandler, ToolEntry } from "../types.js";
import {
  CoreErrorType,
  READ_DEFAULT_MAX_LINES,
  READ_MAX_FILE_SIZE_BYTES,
  READ_MAX_OUTPUT_TOKENS,
  WriteInputJsonSchema,
  WriteInputSchema,
  WriteOutputJsonSchema,
  WriteOutputSchema,
  createCoreError,
  isFileSystemPortError,
  type FileSystemPort,
  type FileSystemReadTextResult,
  type FileSystemTextEncoding,
  type WriteInput,
  type TraceContext,
} from "@zcode/contracts";
import { estimateTokens } from "../../context/utils.js";
import { createStructuredPatch } from "../diff.js";
import { addReadLineNumbers } from "./read-text.js";
import { stampMemoryOriginSessionId } from "../../memory/origin-session.js";
import { resolveWorkspacePath } from "../path-policy.js";
import {
  findLatestReadFileState,
  normalizeReadFileStateMtimeMs,
  requestedPathField,
  setReadFileStateEntry,
} from "../read-file-state.js";
import { createReadFileStateMetadataFromEntry } from "../read-file-state-metadata.js";
import type { ReadFileStateEntry, ReadFileStateMap, ToolExecutionContext } from "../types.js";
import {
  attachToolExecutionTelemetry,
  elapsedMsSince,
  fileByteCount,
  workspaceKind,
} from "./tool-perf.js";

const WRITE_PROVIDER_DESCRIPTION = [
  "Writes a file to the local filesystem, overwriting if one exists.",
  "",
  "When to use: creating a new file, or fully replacing one you've already Read. Overwriting an existing file you haven't Read will fail. For partial changes, use Edit instead.",
].join("\n");

const WRITE_STORAGE_SUFFIX =
  " (file state is current in your context \u2014 no need to Read it back)";
const WRITE_USER_MODIFIED_NOTE = " The user modified your proposed content before accepting it.";
const WRITE_NOT_READ_MESSAGE = "File has not been read yet. Read it first before writing to it.";
const WRITE_STALE_MESSAGE =
  "File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.";

function formatWriteModelContent(output: unknown): string {
  if (!isRecord(output)) {
    return "The file has been written successfully.";
  }

  const filePath = typeof output.filePath === "string" ? output.filePath : "the file";
  const userModified = output.userModified === true;
  const userModifiedNote = userModified ? WRITE_USER_MODIFIED_NOTE : "";
  const storageSuffixNote = userModified ? "" : WRITE_STORAGE_SUFFIX;

  if (output.type === "create") {
    return `File created successfully at: ${filePath}${userModifiedNote}${storageSuffixNote}`;
  }

  return `The file ${filePath} has been updated successfully.${userModifiedNote}${storageSuffixNote}`;
}

export const writeHandler: ToolHandler = async (input, context) => {
  const { file_path, content } = WriteInputSchema.parse(input) as WriteInput;
  const fileSystemPort = context.fileSystemPort;

  if (!fileSystemPort) {
    throw createCoreError(
      CoreErrorType.ConfigurationError,
      "FileSystemPort is not configured for Write tool",
      {
        context: {
          toolCallId: context.toolCallId,
          toolName: "Write",
        },
        recoverable: false,
      },
    );
  }

  const filePath = resolveWorkspacePath({
    inputPath: file_path,
    operation: "write",
    workingDirectory: context.workingDirectory,
    workspaceRoot: context.workspaceRoot,
  });

  let originalFile: string | null = null;
  let originalEncoding: FileSystemTextEncoding | undefined;
  let originalLineEndings: "LF" | "CRLF" | undefined;
  let originalRevision: FileSystemReadTextResult["revision"] | undefined;
  const readStartedAt = Date.now();
  let fsReadMs = 0;
  try {
    const read = await fileSystemPort.readTextFile(
      {
        path: filePath,
        trace: createWriteTrace(context),
      },
      { signal: context.abortSignal },
    );
    const rejection = checkWritableExistingFile(filePath, read, context.readFileState);
    if (rejection) {
      throw await createWriteRejectionError(rejection, {
        context,
        filePath,
        read,
        requestedPath: file_path,
      });
    }
    originalFile = read.content;
    originalEncoding = read.encoding;
    originalLineEndings = read.lineEndings;
    originalRevision = read.revision;
    fsReadMs = elapsedMsSince(readStartedAt);
  } catch (error) {
    fsReadMs = elapsedMsSince(readStartedAt);
    if (!isFileSystemPortError(error) || error.code !== "not_found") {
      throw error;
    }
  }

  const contentToWrite = stampMemoryOriginSessionId({
    content,
    filePath,
    memoryRoot: context.memoryRoot,
    sessionId: context.sessionId,
  });
  const writeStartedAt = Date.now();
  const writeResult = await fileSystemPort.writeTextFile(
    {
      path: filePath,
      content: contentToWrite,
      encoding: originalEncoding,
      lineEndings: originalLineEndings,
      createParents: true,
      atomic: true,
      expectedRevision: originalRevision,
      trace: createWriteTrace(context),
    },
    { signal: context.abortSignal },
  );
  const fsWriteMs = elapsedMsSince(writeStartedAt);

  const readFileStateEntry = updateReadFileStateAfterWrite(
    context.readFileState,
    filePath,
    file_path,
    contentToWrite,
    writeResult.revision,
  );
  recordReadFileStateMetadata(context, readFileStateEntry);

  if (originalFile) {
    return attachToolExecutionTelemetry(
      {
        type: "update",
        filePath: file_path,
        content: contentToWrite,
        structuredPatch: createStructuredPatch({
          filePath: file_path,
          oldContent: originalFile,
          newContent: contentToWrite,
        }),
        originalFile,
        userModified: false,
      },
      createWritePerformanceTelemetry({
        content: contentToWrite,
        fsReadMs,
        fsWriteMs,
        context,
      }),
    );
  }

  return attachToolExecutionTelemetry(
    {
      type: "create",
      filePath: file_path,
      content: contentToWrite,
      structuredPatch: [],
      originalFile: null,
      userModified: false,
    },
    createWritePerformanceTelemetry({
      content: contentToWrite,
      fsReadMs,
      fsWriteMs,
      context,
    }),
  );
};

function createWritePerformanceTelemetry(input: {
  content: string;
  context: ToolExecutionContext;
  fsReadMs: number;
  fsWriteMs: number;
}) {
  const totalBytes = fileByteCount(input.content);
  return {
    detail: {
      kind: "filesystem" as const,
      filesystem: {
        readMs: input.fsReadMs,
        writeMs: input.fsWriteMs,
        fileCount: 1,
        totalBytes,
        maxFileBytes: totalBytes,
        workspaceKind: workspaceKind(input.context),
      },
    },
  };
}

export const writeToolEntry: ToolEntry = {
  capability: "Create or overwrite a file through the file-system adapter",
  metadata: {
    name: "Write",
    description: WRITE_PROVIDER_DESCRIPTION,
    readOnly: false,
    destructive: false,
    concurrentSafe: false,
    timeoutMs: 30000,
    maxOutputBytes: 1_000_000,
    sideEffectScope: "workspace",
    riskLevel: "medium",
    needsApproval: true,
  },
  handler: writeHandler,
  formatModelContent: formatWriteModelContent,
  inputSchema: WriteInputJsonSchema,
  outputSchema: WriteOutputJsonSchema,
  runtimeInputSchema: WriteInputSchema,
  runtimeOutputSchema: WriteOutputSchema,
  permission: {
    permission: "edit",
    reason: "Write creates or overwrites files through the file-system adapter",
    riskLevel: "medium",
    sideEffectScope: "workspace",
    needsApproval: true,
    patternSources: ["path"],
    alwaysAllowPatternSources: ["path"],
    denyPriority: "beforeAsk",
  },
  resultBudget: {
    maxInlineBytes: 1_000_000,
    maxModelBytes: 100_000,
    strategy: "truncate",
    preview: {
      maxBytes: 100_000,
      direction: "head",
    },
  },
  timeout: {
    defaultMs: 30000,
    maxMs: 30000,
    allowCallOverride: false,
  },
  cancellation: {
    supported: true,
    cleanup: "bestEffort",
    userVisibleMessage: "Write was cancelled before the file operation completed",
  },
  trace: {
    required: true,
    propagateToAdapters: true,
    recordInput: "summary",
    recordOutput: "summary",
  },
};

function createWriteTrace(context: ToolExecutionContext): TraceContext {
  return {
    traceId: context.traceId,
    spanId: context.spanId,
    parentSpanId: context.parentSpanId,
    sessionId: context.sessionId,
    turnId: context.turnId,
  } as unknown as TraceContext;
}

interface WriteRejection {
  code: "write_file_not_read" | "write_file_stale";
  message: string;
}

/**
 * 流中截断判定（docs/design/v2/tool/17-read-file-state-resume.md「未读 Write 在流中截断」）：
 * true 表示这次 Write 一定会以 write_file_not_read 被拒——文件已存在，本会话没有完整视图记录。
 * 任何不确定（路径越界、stat 失败、不存在）都返回 false，让模型照常写完。
 */
export async function isUnreadExistingWriteTarget(input: {
  filePath: string;
  fileSystemPort: FileSystemPort | undefined;
  readFileState: ReadFileStateMap;
  signal?: AbortSignal;
  workingDirectory: string;
  workspaceRoot: string;
}): Promise<boolean> {
  if (!input.fileSystemPort) return false;
  try {
    const resolved = resolveWorkspacePath({
      inputPath: input.filePath,
      operation: "write",
      workingDirectory: input.workingDirectory,
      workspaceRoot: input.workspaceRoot,
    });
    // 与 checkWritableExistingFile 的 write_file_not_read 条件相同。
    const lastRead = findLatestReadFileState(input.readFileState, resolved);
    if (lastRead && !lastRead.isPartialView) return false;
    const stat = await input.fileSystemPort.stat({ path: resolved }, { signal: input.signal });
    return stat.kind === "file";
  } catch {
    return false;
  }
}

function checkWritableExistingFile(
  filePath: string,
  currentRead: FileSystemReadTextResult,
  readFileState: ReadFileStateMap | undefined,
): WriteRejection | undefined {
  const lastRead = findLatestReadFileState(readFileState, filePath);
  if (!lastRead || lastRead.isPartialView) {
    return { code: "write_file_not_read", message: WRITE_NOT_READ_MESSAGE };
  }

  if (!hasReadStateChanged(lastRead, currentRead)) return undefined;
  if (isStrictFullRead(lastRead) && lastRead.content === currentRead.content) return undefined;

  return { code: "write_file_stale", message: WRITE_STALE_MESSAGE };
}

async function createWriteRejectionError(
  rejection: WriteRejection,
  input: {
    context: ToolExecutionContext;
    filePath: string;
    read: FileSystemReadTextResult;
    requestedPath: string;
  },
): Promise<Error> {
  const modelContentSuffix = await adoptCurrentContentAsRead(input);
  return createCoreError(CoreErrorType.ToolExecutionFailed, rejection.message, {
    context: {
      code: rejection.code,
      filePath: input.filePath,
      ...(modelContentSuffix === undefined ? {} : { modelContentSuffix }),
    },
    recoverable: true,
  });
}

/**
 * 修复原因：被拒的 Write 已经读到了当前内容，原来只回一句报错，模型要再 Read 一次、再完整生成一次
 * Write（整份文件的 output token 白付一遍，外加一个往返）。这里把正文按 Read 的格式附在 model-only
 * 后缀里，并记成完整视图，模型可以直接重试。只在 Read 同一路径不会被拒、且文件在 Read 的上限内时这样做，
 * 否则保持原报错（见 docs/design/v2/tool/17-read-file-state-resume.md）。
 */
async function adoptCurrentContentAsRead(input: {
  context: ToolExecutionContext;
  filePath: string;
  read: FileSystemReadTextResult;
  requestedPath: string;
}): Promise<string | undefined> {
  const { context, filePath, read, requestedPath } = input;
  const readFileState = context.readFileState;
  if (!readFileState || !context.areReadsDeniedWithoutPrompt) return undefined;

  const sizeBytes = read.revision?.sizeBytes ?? Buffer.byteLength(read.content, "utf8");
  if (
    sizeBytes > READ_MAX_FILE_SIZE_BYTES ||
    read.content.split(/\r?\n/).length > READ_DEFAULT_MAX_LINES ||
    estimateTokens(read.content) > READ_MAX_OUTPUT_TOKENS
  ) {
    return undefined;
  }

  // 读权限规则按原始字符串匹配：规范化路径和原始写法各判定一次，任一 deny 就不附带。
  const forms = requestedPath === filePath ? [filePath] : [filePath, requestedPath];
  let denied: boolean[];
  try {
    denied = await context.areReadsDeniedWithoutPrompt(forms);
  } catch {
    return undefined;
  }
  if (denied.length !== forms.length || denied.includes(true)) return undefined;

  setReadFileStateEntry(readFileState, {
    path: filePath,
    ...requestedPathField(requestedPath, filePath),
    content: read.content,
    offset: undefined,
    limit: undefined,
    isPartialView: false,
    readAt: new Date(),
    sourceTool: "Read",
    revisionId: read.revision?.id,
    mtimeMs: normalizeReadFileStateMtimeMs(read.revision?.mtimeMs),
    sizeBytes,
  });

  if (read.content.length === 0) {
    return "The file exists and is currently empty. It now counts as read, so you can retry Write without calling Read.";
  }
  return [
    "Its current contents are below (same format as Read) and now count as read, so you can retry Write or Edit without calling Read:",
    "",
    addReadLineNumbers({ content: read.content, startLine: 1 }),
  ].join("\n");
}

function hasReadStateChanged(
  lastRead: ReadFileStateEntry,
  currentRead: FileSystemReadTextResult,
): boolean {
  const currentRevisionId = currentRead.revision?.id;
  if (lastRead.revisionId && currentRevisionId && lastRead.revisionId !== currentRevisionId) {
    return true;
  }

  const currentMtimeMs = currentRead.revision?.mtimeMs;
  if (lastRead.mtimeMs !== undefined && currentMtimeMs !== undefined) {
    // 让已确认内容未变的 Write 被误判为 stale。
    const normalizedCurrentMtimeMs = normalizeReadFileStateMtimeMs(currentMtimeMs);
    const normalizedLastReadMtimeMs = normalizeReadFileStateMtimeMs(lastRead.mtimeMs);
    const mtimeAdvanced =
      normalizedCurrentMtimeMs !== undefined &&
      normalizedLastReadMtimeMs !== undefined &&
      normalizedCurrentMtimeMs > normalizedLastReadMtimeMs;
    return mtimeAdvanced || lastRead.sizeBytes !== currentRead.sizeBytes;
  }

  if (lastRead.sizeBytes !== undefined && lastRead.sizeBytes !== currentRead.sizeBytes) {
    return true;
  }

  return isStrictFullRead(lastRead) && lastRead.content !== currentRead.content;
}

function isStrictFullRead(entry: ReadFileStateEntry): boolean {
  if (entry.isPartialView) return false;
  return (entry.offset ?? 1) <= 1 && entry.limit === undefined;
}

function updateReadFileStateAfterWrite(
  readFileState: ReadFileStateMap | undefined,
  filePath: string,
  requestedPath: string,
  content: string,
  revision: FileSystemReadTextResult["revision"] | undefined,
): ReadFileStateEntry | undefined {
  if (!readFileState) return undefined;
  const entry: ReadFileStateEntry = {
    path: filePath,
    ...requestedPathField(requestedPath, filePath),
    content,
    offset: undefined,
    limit: undefined,
    isPartialView: false,
    readAt: new Date(),
    sourceTool: "Write",
    revisionId: revision?.id,
    mtimeMs: normalizeReadFileStateMtimeMs(revision?.mtimeMs),
    sizeBytes: revision?.sizeBytes ?? Buffer.byteLength(content, "utf8"),
  };
  setReadFileStateEntry(readFileState, entry);
  return entry;
}

function recordReadFileStateMetadata(
  context: ToolExecutionContext,
  entry: ReadFileStateEntry | undefined,
): void {
  if (!context.recordReadFileStateMetadata) return;
  const metadata = createReadFileStateMetadataFromEntry({
    completedAt: entry?.readAt ?? new Date(),
    entry,
    toolName: "Write",
  });
  if (metadata) context.recordReadFileStateMetadata(metadata);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
