import { afterEach, describe, expect, it, vi } from "vitest";
import { replaceSecretVault, secretVaultEnv } from "@zcode/core";
import type { ExecutionPort, ExecutionRequest } from "@zcode/contracts";
import { zcodeSecretsSyncResultSchema } from "@zcode/shared";
import { syncSecrets } from "../src/zcode-protocol/secrets-sync.js";
import { withOfficialCommandEnv } from "../src/official-command-endpoint/index.js";
import type { ZCodeProtocolAgentServerContext } from "../src/zcode-protocol/server-types.js";

const workspace = { workspacePath: "/workspace/app", workspaceKey: "/workspace/app" };

function context() {
  const info = vi.fn();
  return {
    context: { logger: { info } } as unknown as ZCodeProtocolAgentServerContext,
    info,
  };
}

describe("secrets/sync", () => {
  afterEach(() => replaceSecretVault([]));

  it("replaces the vault and logs the count only", async () => {
    const { context: ctx, info } = context();
    const result = await syncSecrets(ctx, {
      workspace,
      secrets: [{ name: "STEAM_TOKEN", value: "steam-value-1" }],
    });
    expect(zcodeSecretsSyncResultSchema.parse(result)).toEqual({ count: 1 });
    expect(secretVaultEnv()).toEqual({ STEAM_TOKEN: "steam-value-1" });
    expect(JSON.stringify(info.mock.calls)).not.toContain("steam-value-1");

    await syncSecrets(ctx, { workspace, secrets: [] });
    expect(secretVaultEnv()).toEqual({});
  });

  it.each([
    ["an invalid name", [{ name: "1BAD", value: "value-123" }]],
    ["a reserved name", [{ name: "path", value: "value-123" }]],
    ["a ZCODE_ name", [{ name: "ZCODE_HOME", value: "value-123" }]],
    ["an empty value", [{ name: "TOKEN", value: "" }]],
    [
      "a duplicate name",
      [
        { name: "TOKEN", value: "value-123" },
        { name: "token", value: "value-456" },
      ],
    ],
  ])("rejects %s and keeps the previous table", async (_label, secrets) => {
    const { context: ctx } = context();
    await syncSecrets(ctx, { workspace, secrets: [{ name: "KEEP", value: "keep-value" }] });
    await expect(syncSecrets(ctx, { workspace, secrets })).rejects.toThrow();
    expect(secretVaultEnv()).toEqual({ KEEP: "keep-value" });
  });
});

describe("secret env on the session execution port", () => {
  afterEach(() => replaceSecretVault([]));

  function capturingPort() {
    const requests: ExecutionRequest[] = [];
    const port: ExecutionPort = {
      run: async (request) => {
        requests.push(request);
        return { exitCode: 0, stdout: "", stderr: "" } as never;
      },
    };
    return { port, requests };
  }

  it("reads the vault per command and skips clean-env requests", async () => {
    const { port, requests } = capturingPort();
    const wrapped = withOfficialCommandEnv(port, async () => secretVaultEnv());
    const request = { command: { mode: "shell", command: "env" } } as unknown as ExecutionRequest;

    await wrapped.run(request);
    expect(requests[0]).toBe(request);

    replaceSecretVault([{ name: "TOKEN", value: "token-value" }]);
    await wrapped.run(request);
    expect(requests[1]?.env?.set).toEqual({ TOKEN: "token-value" });

    await wrapped.run({ ...request, env: { base: "empty" } } as ExecutionRequest);
    expect(requests[2]?.env).toEqual({ base: "empty" });
  });
});
