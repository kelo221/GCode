import { replaceSecretVault } from "@zcode/core";
import { zcodeSecretsSyncParamsSchema, type ZCodeSecretsSyncResult } from "@zcode/shared";
import { parseParams, type ZCodeProtocolAgentServerContext } from "./server-types.js";

/**
 * GPUI 专属密钥库整表同步（apps/zcode-gpui/SECRETS.md）。
 *
 * GPUI 每个 workspace 一个 agent 进程，已按 workspace 过滤，这里直接整表替换进程级密钥库。
 * 校验失败整表拒绝，旧表继续生效。值只在内存；日志只记条数。
 */
export async function syncSecrets(
  context: ZCodeProtocolAgentServerContext,
  rawParams: unknown,
): Promise<ZCodeSecretsSyncResult> {
  const params = parseParams(zcodeSecretsSyncParamsSchema, rawParams);
  replaceSecretVault(params.secrets);
  context.logger?.info("Secrets vault synced", {
    count: params.secrets.length,
    event: "zcode_protocol.secrets.synced",
  });
  return { count: params.secrets.length };
}
