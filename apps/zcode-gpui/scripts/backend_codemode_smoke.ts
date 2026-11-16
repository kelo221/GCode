// End-to-end check of the built-in codemode tool (apps/zcode-cli/docs/design/v2/tool/21-codemode.md).
// A mock model calls codemode with a script that reads a file through tools.Read and stores a
// value, then answers. The run asserts the tool exposure per codemodeMode (off / on / only) and
// that the script's nested Read result reaches the model. Against the compiled backend this is
// the check that the embedded worker and the staged quickjs.wasm load.
//
//   bun apps/zcode-gpui/scripts/backend_codemode_smoke.ts
//   ZCODE_GPUI_SMOKE_BACKEND=apps/zcode-gpui/target/backend/zcode-backend.exe bun ...
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { commandAckSchema, commandEnvelopeSchema } from "@zcode/shared/zcode-protocol-v4";
import { smokeProviderConfig } from "./backend_mock.js";

const root = resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const modelSelection = {
  providerId: "smoke-openai",
  modelId: "mock-chat",
  options: { reasoningLevel: "low" },
};
const SCRIPT = [
  '// @options: {"max_output_tokens": 2000}',
  "const text = await tools.Read({ file_path: 'notes.txt' });",
  "store('seen', true);",
  "console.log(text.includes('CODEMODE_NEEDLE_7') ? 'FOUND_NEEDLE' : 'MISSING:' + text.slice(0, 200));",
].join("\n");

type Mode = "off" | "on" | "only";
type Body = { tools?: Array<{ function?: { name?: string } }>; messages?: unknown[] };

async function mockModel() {
  const bodies: Body[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) chunks.push(Buffer.from(chunk));
    const body = JSON.parse(Buffer.concat(chunks).toString()) as Body;
    bodies.push(body);
    const toolTurns = (body.messages ?? []).filter(
      (m) => (m as { role?: string }).role === "tool",
    ).length;
    const offered = new Set((body.tools ?? []).map((t) => t.function?.name));
    const delta =
      toolTurns === 0 && offered.has("codemode")
        ? {
            role: "assistant",
            tool_calls: [
              {
                index: 0,
                id: "call_0",
                type: "function",
                function: { name: "codemode", arguments: JSON.stringify({ code: SCRIPT }) },
              },
            ],
          }
        : { role: "assistant", content: "ZCODE_SMOKE_OK" };
    const frames = [
      { id: "m", object: "chat.completion.chunk", choices: [{ index: 0, delta }] },
      {
        id: "m",
        object: "chat.completion.chunk",
        choices: [
          { index: 0, delta: {}, finish_reason: "tool_calls" in delta ? "tool_calls" : "stop" },
        ],
        usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
      },
    ];
    response
      .writeHead(200, { "content-type": "text/event-stream" })
      .end(frames.map((f) => `data: ${JSON.stringify(f)}\n\n`).join("") + "data: [DONE]\n\n");
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("no mock port");
  return { server, port: address.port, bodies };
}

async function run(mode: Mode) {
  const scratch = await mkdtemp(join(tmpdir(), "zcode-gpui-codemode-"));
  const mock = await mockModel();
  const home = join(scratch, "home");
  const workspace = join(scratch, "workspace");
  await mkdir(workspace, { recursive: true });
  await mkdir(home, { recursive: true });
  await writeFile(join(workspace, "notes.txt"), "line one\nCODEMODE_NEEDLE_7\n");
  const providerFile = join(scratch, "provider_config.json");
  const provider = smokeProviderConfig(modelSelection, mock.port);
  provider.config.modelConfigRules.providerModelRules[0]!.config.properties.contextWindow = 200_000;
  await writeFile(providerFile, JSON.stringify(provider));
  const compiled =
    process.env.ZCODE_GPUI_SMOKE_BACKEND && resolve(process.env.ZCODE_GPUI_SMOKE_BACKEND);
  const serverArgs = ["app-server", "--stdio", "--surface", "desktop"];
  const child = spawn(
    compiled ?? process.execPath,
    compiled ? serverArgs : [join(root, "apps/zcode-cli/packages/cli/src/main.ts"), ...serverArgs],
    {
      cwd: workspace,
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        PATH: process.env.PATH,
        SystemRoot: process.env.SystemRoot,
        WINDIR: process.env.WINDIR,
        TEMP: scratch,
        TMP: scratch,
        HOME: home,
        USERPROFILE: home,
        APPDATA: home,
        LOCALAPPDATA: home,
        ZCODE_RUNTIME_ENV: "desktop",
        ZCODE_DESKTOP_HOME_DIR: home,
        ZCODE_DATA_BASE_DIR: join(scratch, "data"),
        ZCODE_STORAGE_DIR: join(scratch, "storage"),
        ZCODE_SESSION_DB_PATH: join(scratch, "session.db"),
        ZCODE_BUILTIN_PROVIDER_CONFIG_FILE: join(root, "config/provider/zcode-builtin.json"),
        ZCODE_PERSONAL_PROVIDER_CONFIG_FILE: providerFile,
      },
    },
  );
  let stderr = "";
  child.stderr.on("data", (c) => (stderr = (stderr + c.toString()).slice(-8000)));
  const notifications: Array<{ method?: string; params?: { phase?: string } }> = [];
  const pending = new Map<number, (m: { result?: unknown; error?: unknown }) => void>();
  createInterface({ input: child.stdout }).on("line", (line) => {
    let m: {
      id?: number;
      method?: string;
      params?: { phase?: string };
      result?: unknown;
      error?: unknown;
    };
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    if (m.method && m.id !== undefined) {
      const result =
        m.method === "session/requestRuntimePreferences"
          ? {
              result: {
                askUserQuestionAutoResolutionEnabled: true,
                nativeSearchEnhancementsEnabled: true,
                memoryEnabled: false,
                // Omitting the field is the old-host path, which must mean off.
                ...(mode === "off" ? {} : { codemodeMode: mode }),
              },
            }
          : { error: { code: -32603, message: "not available in smoke" } };
      child.stdin.write(JSON.stringify({ id: m.id, ...result }) + "\n");
    } else if (typeof m.id === "number") pending.get(m.id)?.(m);
    else notifications.push(m);
  });
  let nextId = 0;
  const rpc = (method: string, params: unknown) =>
    new Promise<unknown>((done, fail) => {
      const id = ++nextId;
      const timer = setTimeout(() => fail(new Error(`${method} timed out; ${stderr}`)), 60_000);
      pending.set(id, (m) => {
        clearTimeout(timer);
        if (m.error) fail(new Error(JSON.stringify(m.error)));
        else done(m.result);
      });
      child.stdin.write(JSON.stringify({ id, method, params }) + "\n");
    });
  const command = async (type: string, sessionId: string | null, payload: unknown) =>
    commandAckSchema.parse(
      await rpc(
        "v4/command",
        commandEnvelopeSchema.parse({
          commandId: randomUUID(),
          clientId: "gpui-codemode-smoke",
          sessionId,
          type,
          issuedAt: Date.now(),
          payload,
        }),
      ),
    );
  const waitFor = async (predicate: () => boolean, label: string, timeout = 60_000) => {
    const end = Date.now() + timeout;
    while (!predicate()) {
      if (child.exitCode !== null) throw new Error(`${label}: backend exited; ${stderr}`);
      if (Date.now() > end) throw new Error(`${label}: timed out; ${stderr}`);
      await sleep(50);
    }
  };
  try {
    await waitFor(
      () =>
        notifications.some(
          (m) => m.method === "startup/storageState" && m.params?.phase === "ready",
        ),
      "storage startup",
    );
    const created = await command("createSession", null, {
      workspaceId: workspace,
      config: { modelSelection, mode: "yolo" },
    });
    if (created.status !== "accepted" || created.result?.type !== "createSession")
      throw new Error(`create: ${JSON.stringify(created)}`);
    const sent = await command("sendText", created.result.sessionId, {
      text: "Read the notes with a script",
      requestedDelivery: "startNow",
      modelSelection,
    });
    if (sent.status !== "accepted") throw new Error(`send: ${JSON.stringify(sent)}`);
    await waitFor(() => mock.bodies.length >= (mode === "off" ? 1 : 2), "model requests");
    await sleep(500);
    const offered = (mock.bodies[0]?.tools ?? []).map((t) => t.function?.name);
    const hasCodemode = offered.includes("codemode");
    const hasRead = offered.includes("Read");
    const expected = { off: [false, true], on: [true, true], only: [true, false] }[mode];
    if (hasCodemode !== expected[0] || hasRead !== expected[1])
      throw new Error(`${mode}: codemode=${hasCodemode} Read=${hasRead}: ${offered.join(",")}`);
    const result: Record<string, unknown> = { mode, codemode: hasCodemode, read: hasRead };
    if (mode !== "off") {
      const toolResults = mock.bodies
        .flatMap((body) => body.messages ?? [])
        .filter((m) => (m as { role?: string }).role === "tool")
        .map((m) => JSON.stringify((m as { content?: unknown }).content));
      if (!toolResults.some((text) => text.includes("FOUND_NEEDLE")))
        throw new Error(`script did not read the file: ${toolResults.join(" | ")}; ${stderr}`);
      result.nestedRead = true;
    }
    return result;
  } finally {
    child.stdin.end();
    if (child.exitCode === null) {
      child.kill();
      await Promise.race([new Promise((done) => child.once("exit", done)), sleep(3000)]);
    }
    mock.server.close();
    await rm(scratch, { recursive: true, force: true }).catch(() => undefined);
  }
}

const results = [];
for (const mode of ["off", "on", "only"] as const) results.push(await run(mode));
console.log(JSON.stringify({ result: "passed", results }));
