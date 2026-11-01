import { afterEach, describe, expect, it } from "vitest";
import {
  redactSecretText,
  redactSecretsDeep,
  replaceSecretVault,
  secretVaultEnv,
  secretVaultSize,
} from "../src/secrets/secret-vault.js";

describe("secret vault", () => {
  afterEach(() => replaceSecretVault([]));

  it("is a no-op while empty", () => {
    const payload = { stdout: "token abc123456" };
    expect(secretVaultEnv()).toEqual({});
    expect(redactSecretText("abc123456")).toBe("abc123456");
    expect(redactSecretsDeep(payload)).toBe(payload);
  });

  it("replaces the whole table on every sync", () => {
    replaceSecretVault([
      { name: "A_TOKEN", value: "aaaaaaaa" },
      { name: "B_TOKEN", value: "bbbbbbbb" },
    ]);
    replaceSecretVault([{ name: "B_TOKEN", value: "cccccccc" }]);
    expect(secretVaultEnv()).toEqual({ B_TOKEN: "cccccccc" });
    expect(secretVaultSize()).toBe(1);
    expect(redactSecretText("aaaaaaaa cccccccc")).toBe("aaaaaaaa ***");
  });

  it("returns a copy of the env so callers cannot mutate the vault", () => {
    replaceSecretVault([{ name: "A_TOKEN", value: "aaaaaaaa" }]);
    const env = secretVaultEnv();
    env.A_TOKEN = "changed";
    expect(secretVaultEnv()).toEqual({ A_TOKEN: "aaaaaaaa" });
  });

  it("masks longer values first and skips values shorter than 6 characters", () => {
    replaceSecretVault([
      { name: "SHORT", value: "abc" },
      { name: "INNER", value: "secret" },
      { name: "OUTER", value: "secret-and-more" },
    ]);
    expect(redactSecretText("abc secret-and-more secret")).toBe("abc *** ***");
  });

  it("redacts nested strings and keeps untouched branches by identity", () => {
    replaceSecretVault([{ name: "TOKEN", value: "s3cr3t-value" }]);
    const untouched = { exitCode: 0, note: "ok" };
    const payload = {
      untouched,
      chunks: ["line s3cr3t-value", 3],
      nested: { stderr: "Bearer s3cr3t-value" },
    };
    const redacted = redactSecretsDeep(payload);
    expect(redacted).toEqual({
      untouched,
      chunks: ["line ***", 3],
      nested: { stderr: "Bearer ***" },
    });
    expect(redacted.untouched).toBe(untouched);
    expect(payload.nested.stderr).toBe("Bearer s3cr3t-value");
  });

  it("leaves non-plain objects alone and survives cycles", () => {
    replaceSecretVault([{ name: "TOKEN", value: "s3cr3t-value" }]);
    const date = new Date(0);
    const cyclic: Record<string, unknown> = { text: "s3cr3t-value", date };
    cyclic.self = cyclic;
    const redacted = redactSecretsDeep(cyclic) as Record<string, unknown>;
    expect(redacted.text).toBe("***");
    expect(redacted.date).toBe(date);
  });
});
