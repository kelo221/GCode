// GCode 密钥库（apps/zcode-gpui/SECRETS.md）：stdio MCP 拉起时现取宿主 env，
// server 自己的 config.env 同名优先。
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createMcpAdapter } from "../src/mcp/index.js";

const roots: string[] = [];

afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { force: true, recursive: true })));
});

async function envDumpServer() {
  const root = await mkdtemp(join(tmpdir(), "zcode-mcp-extra-env-"));
  roots.push(root);
  const serverPath = join(root, "server.mjs");
  const outFile = join(root, "env.json");
  const mcpServerModule = import.meta.resolve("@modelcontextprotocol/server");
  const stdioModule = import.meta.resolve("@modelcontextprotocol/server/stdio");
  await writeFile(
    serverPath,
    [
      "import { writeFileSync } from 'node:fs';",
      `import { McpServer } from ${JSON.stringify(mcpServerModule)};`,
      `import { StdioServerTransport } from ${JSON.stringify(stdioModule)};`,
      "const { SECRET_A, OVERRIDE } = process.env;",
      "writeFileSync(process.env.OUT_FILE, JSON.stringify({ SECRET_A, OVERRIDE }));",
      "const server = new McpServer({ name: 'env-dump', version: '1.0.0' });",
      "await server.connect(new StdioServerTransport());",
    ].join("\n"),
  );
  return { outFile, serverPath };
}

describe("extraStdioEnv", () => {
  it("adds host env to stdio servers, read at spawn, below the server's own env", async () => {
    const { outFile, serverPath } = await envDumpServer();
    let vault: Record<string, string> = { SECRET_A: "stale" };
    const adapter = createMcpAdapter({ extraStdioEnv: () => vault });
    vault = { SECRET_A: "from-vault", OVERRIDE: "vault" };

    try {
      await expect(
        adapter.connectServer("env-dump", {
          args: [serverPath],
          command: process.execPath,
          env: { OUT_FILE: outFile, OVERRIDE: "config" },
          timeoutMs: 10_000,
          type: "stdio",
        }),
      ).resolves.toMatchObject({ status: "connected" });

      expect(JSON.parse(await readFile(outFile, "utf8"))).toEqual({
        SECRET_A: "from-vault",
        OVERRIDE: "config",
      });
    } finally {
      await adapter.close();
    }
  });
});
