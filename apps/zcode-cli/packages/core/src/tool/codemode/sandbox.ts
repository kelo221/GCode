// ============================================================
// codemode sandbox (process singleton)
// ============================================================
// 一个进程一个 CodemodeSandbox；每次执行一个 worker 线程 + 一个 QuickJS VM（pi-codemode）。
// 编译后的 backend exe 里，quickjs.wasm 拷在 exe 同级 node_modules/quickjs-wasi/，worker 是嵌入的
// 额外 build entrypoint（apps/zcode-gpui/scripts/build_backend.ts）。

import { dirname, join } from "node:path";
import {
  CodemodeSandbox,
  loadQuickJSWasm,
  type CodemodeExecuteOptions,
  type CodemodeResult,
  type CodemodeTool,
} from "@earendil-works/pi-codemode";

/** build_backend.ts 嵌入 worker 时使用的 entrypoint 名；两侧必须一致。 */
export const CODEMODE_COMPILED_WORKER_SPECIFIER = "./codemode-worker.js";
const VM_MEMORY_LIMIT_BYTES = 256 * 1024 * 1024;
const DEFAULT_MAX_WORKERS = 2;
const WORKER_WAIT_MS = 60_000;

function isCompiledBunBinary(): boolean {
  const main = (globalThis as { Bun?: { main?: string } }).Bun?.main ?? "";
  return /^(?:B:[\\/]~BUN[\\/]|\/\$bunfs\/)/.test(main);
}

function maxWorkers(): number {
  const raw = Number(process.env.GCODE_CODEMODE_MAX_WORKERS);
  return Number.isInteger(raw) && raw > 0 ? raw : DEFAULT_MAX_WORKERS;
}

let running = 0;
const waiting: Array<() => void> = [];

/** 每进程最多 maxWorkers() 个脚本同时运行；按到达顺序排队，最多等 60 秒。 */
async function acquireSlot(signal: AbortSignal): Promise<() => void> {
  if (running >= maxWorkers()) {
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        cleanup();
        reject(new Error("codemode is busy: other scripts are still running. Try again shortly."));
      }, WORKER_WAIT_MS);
      const onAbort = () => {
        cleanup();
        reject(signal.reason ?? new Error("codemode was cancelled"));
      };
      const wake = () => {
        cleanup();
        resolve();
      };
      const cleanup = () => {
        clearTimeout(timer);
        signal.removeEventListener("abort", onAbort);
        const index = waiting.indexOf(wake);
        if (index >= 0) waiting.splice(index, 1);
      };
      signal.addEventListener("abort", onAbort, { once: true });
      waiting.push(wake);
    });
  }
  running += 1;
  return () => {
    running -= 1;
    waiting[0]?.();
  };
}

function createSandboxOptions() {
  if (!isCompiledBunBinary()) return {};
  const exeDir = dirname(process.execPath);
  return {
    wasm: loadQuickJSWasm(join(exeDir, "node_modules", "quickjs-wasi", "quickjs.wasm")),
    workerUrl: CODEMODE_COMPILED_WORKER_SPECIFIER,
  };
}

export interface RunCodemodeScriptInput {
  code: string;
  tools: CodemodeTool[];
  globals: CodemodeTool[];
  options: CodemodeExecuteOptions & { signal: AbortSignal };
}

/**
 * 工具与 globals 每次调用都不同（嵌套端口绑定当前调用），所以每次构造一个 sandbox 对象；
 * 它很轻，wasm 编译结果由 loadQuickJSWasm 按路径缓存。
 */
export async function runCodemodeScript(input: RunCodemodeScriptInput): Promise<CodemodeResult> {
  const release = await acquireSlot(input.options.signal);
  // 修复原因：构造器会同步抛错（重名工具、非法 global 名等）；原先构造在 try 之外，
  // 抛错时槽位永不归还，两次失败后整个进程的 codemode 都卡在排队上。
  let sandbox: CodemodeSandbox | undefined;
  try {
    sandbox = new CodemodeSandbox({
      ...createSandboxOptions(),
      tools: input.tools,
      globals: input.globals,
      timeoutMs: Number.POSITIVE_INFINITY,
      memoryLimitBytes: VM_MEMORY_LIMIT_BYTES,
    });
    return await sandbox.execute(input.code, input.options);
  } finally {
    release();
    await sandbox?.close().catch(() => undefined);
  }
}
