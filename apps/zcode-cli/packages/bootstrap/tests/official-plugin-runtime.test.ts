import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { afterEach, describe, expect, it } from "vitest";
import type { McpServerConfig } from "@zcode/contracts";
import {
  bindPluginHostServersToCurrentRuntime,
  writeOfficialPluginRuntimeManifest,
} from "../src/app/official-plugin-runtime.js";

describe("official plugin runtime manifest", () => {
  const cleanups: string[] = [];
  const originalEntrypoint = process.argv[1];

  afterEach(async () => {
    process.argv[1] = originalEntrypoint;
    await Promise.all(cleanups.splice(0).map((root) => rm(root, { force: true, recursive: true })));
  });

  it("does not rewrite an unchanged runtime manifest", async () => {
    const root = await mkdtemp(join(tmpdir(), "zcode-plugin-runtime-manifest-"));
    cleanups.push(root);
    const manifestPath = join(root, ".zcode-plugin", "plugin.json");
    await writeJson(manifestPath, {
      mcpServers: {
        "browser-use": {
          args: [],
          command: "old-node",
        },
      },
      name: "browser-use",
    });
    process.argv[1] = join(root, "zcode.cjs");

    await writeOfficialPluginRuntimeManifest({
      pluginName: "browser-use",
      rootPath: root,
    });
    const expectedContents = await readFile(manifestPath, "utf8");
    const firstStat = await stat(manifestPath);
    await delay(30);

    await writeOfficialPluginRuntimeManifest({
      pluginName: "browser-use",
      rootPath: root,
    });
    const secondStat = await stat(manifestPath);

    expect(await readFile(manifestPath, "utf8")).toBe(expectedContents);
    expect(secondStat.mtimeMs).toBe(firstStat.mtimeMs);
  });

  it("repairs cached entries from the seed and leaves HTTP servers untouched", async () => {
    const root = await mkdtemp(join(tmpdir(), "zcode-plugin-entry-"));
    cleanups.push(root);
    const manifestPath = join(root, ".zcode-plugin", "plugin.json");
    await writeJson(manifestPath, { mcpServers: { pdf: { command: "node", args: ["wrong.js"] } } });
    process.argv[1] = join(root, "zcode.cjs");
    const remote = { type: "http", url: "https://example.com/mcp" };
    const sourceManifestContents = JSON.stringify({
      mcpServers: {
        pdf: { command: "node", args: ["${ZCODE_PLUGIN_ROOT}/dist/server.mjs", "--flag"] },
        search: remote,
      },
    });
    const input = { pluginName: "pdf", rootPath: root, sourceManifestContents };
    await writeOfficialPluginRuntimeManifest(input);
    const contents = await readFile(manifestPath, "utf8");
    const manifest = JSON.parse(contents);
    expect(manifest.mcpServers.pdf.args.slice(-2)).toEqual([
      // ${ZCODE_PLUGIN_ROOT} 按字面替换，保留 manifest 里的 "/"；Windows 上 join 会改成 "\"。
      `${root}/dist/server.mjs`,
      "--flag",
    ]);
    expect(manifest.mcpServers.pdf.env.ELECTRON_RUN_AS_NODE).toBe("1");
    expect(manifest.mcpServers.search).toEqual(remote);
    await writeOfficialPluginRuntimeManifest(input);
    expect(await readFile(manifestPath, "utf8")).toBe(contents);
  });

  it("rebinds plugin-host servers written by another runtime to this process", () => {
    process.argv[1] = join(tmpdir(), "main.ts");
    const servers: Record<string, McpServerConfig> = {
      documents: {
        type: "stdio",
        command: "C:\\Programs\\ZCode\\ZCode.exe",
        args: ["C:\\Programs\\ZCode\\zcode.cjs", "__zcode-plugin-host", "/plugin/dist/server.mjs"],
        env: { ELECTRON_RUN_AS_NODE: "1" },
      },
      user: { type: "stdio", command: "uvx", args: ["blender-mcp"] },
      remote: { type: "http", url: "https://example.com/mcp" },
    };
    const untouched = { user: servers.user, remote: servers.remote };

    bindPluginHostServersToCurrentRuntime(servers);

    expect(servers.documents).toMatchObject({
      command: process.execPath,
      args: [
        ...process.execArgv,
        join(tmpdir(), "main.ts"),
        "__zcode-plugin-host",
        "/plugin/dist/server.mjs",
      ],
      env: { ELECTRON_RUN_AS_NODE: "1" },
    });
    expect({ user: servers.user, remote: servers.remote }).toEqual(untouched);
  });

  it("keeps invalid mcpServers structural errors fatal", async () => {
    const root = await mkdtemp(join(tmpdir(), "zcode-plugin-runtime-invalid-"));
    cleanups.push(root);
    await writeJson(join(root, ".zcode-plugin", "plugin.json"), {
      mcpServers: "invalid",
      name: "browser-use",
    });
    process.argv[1] = join(root, "zcode.cjs");

    await expect(
      (async () =>
        await writeOfficialPluginRuntimeManifest({
          pluginName: "browser-use",
          rootPath: root,
        }))(),
    ).rejects.toThrow("Official plugin manifest has invalid mcpServers.");
  });
});

async function writeJson(path: string, value: unknown): Promise<void> {
  await import("node:fs/promises").then(({ mkdir }) => mkdir(dirname(path), { recursive: true }));
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
}
