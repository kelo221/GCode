// GCode 密钥库（apps/zcode-gpui/SECRETS.md）：工具结果在进模型、事件与 display 之前按值脱敏，
// 失败路径的错误文本同样脱敏。
import {
  ESCALATE_TOOL_NAME,
  createRootTraceContext,
  createSessionId,
  createToolCallId,
  createTurnId,
  type WorkflowEscalatePort,
} from "@zcode/contracts";
import { afterEach, describe, expect, it } from "vitest";
import { PermissionService, defaultPermissionConfig } from "../src/permission/service.js";
import { replaceSecretVault } from "../src/secrets/secret-vault.js";
import { createToolExecutor } from "../src/tool/executor.js";
import { escalateToolEntry } from "../src/tool/handlers/escalate.js";
import { createToolRegistry } from "../src/tool/registry.js";

const SECRET = "s3cr3t-token-value";

function executorWith(port: WorkflowEscalatePort) {
  const sessionId = createSessionId("secret-redaction");
  const turnId = createTurnId("secret-redaction");
  const traceContext = createRootTraceContext({ sessionId, turnId });
  const registry = createToolRegistry();
  registry.register(escalateToolEntry);
  const events: unknown[] = [];
  const executor = createToolExecutor({
    emitEvent: async (event) => {
      events.push(event);
    },
    permissionService: new PermissionService(defaultPermissionConfig),
    registry,
    workflowEscalatePort: port,
    sessionId,
    turnId,
    traceContext,
  });
  const run = () =>
    executor.execute(
      { id: createToolCallId("secret"), input: { question: "token?" }, name: ESCALATE_TOOL_NAME },
      { traceContext },
    );
  return { events, run };
}

describe("tool output secret redaction", () => {
  afterEach(() => replaceSecretVault([]));

  it("masks vault values in the result, model content and emitted events", async () => {
    replaceSecretVault([{ name: "TOKEN", value: SECRET }]);
    const { events, run } = executorWith({
      escalate: async () => ({ kind: "answered", answer: `Use ${SECRET} here`, qid: "q-1" }),
    });

    const result = await run();

    expect(result.success).toBe(true);
    expect(result.modelContent).toBe("Use *** here");
    expect(JSON.stringify(result)).not.toContain(SECRET);
    expect(JSON.stringify(events)).not.toContain(SECRET);
  });

  it("masks vault values in thrown error text", async () => {
    replaceSecretVault([{ name: "TOKEN", value: SECRET }]);
    const { events, run } = executorWith({
      escalate: async () => {
        throw new Error(`upstream rejected ${SECRET}`);
      },
    });

    const result = await run();

    expect(result.success).toBe(false);
    expect(JSON.stringify(result)).not.toContain(SECRET);
    expect(JSON.stringify(result)).toContain("upstream rejected ***");
    expect(JSON.stringify(events)).not.toContain(SECRET);
  });

  it("leaves output untouched while the vault is empty", async () => {
    const { run } = executorWith({
      escalate: async () => ({ kind: "answered", answer: `Use ${SECRET} here`, qid: "q-1" }),
    });
    expect((await run()).modelContent).toBe(`Use ${SECRET} here`);
  });
});
