import { readFile } from "node:fs/promises";
import { generateText } from "ai";
import { beforeAll, describe, expect, it } from "vitest";
import {
  ModelConfigRules,
  createRegistryModelConfig,
  createRegistryProviderConfig,
  parsePersonalModelConfigRules,
  parseProviderConfig,
  parseZCodeBuiltinModelConfigRules,
} from "@zcode/provider";
import { AiSdkModelExecution } from "../src/model/model-execution.js";
import { createGenerateTextOptions } from "../src/model/runner-options.js";
import { createStatusContext } from "../src/model/runner-status.js";
import type { AiSdkModelTextRequest } from "../src/model/runner-runtime.js";

const MISTRAL_URL = "https://api.mistral.ai/v1";
const OTHER_URL = "https://llm.example.test/v1";
const REASONING_FIELDS = ["reasoning_effort", "reasoning", "thinking", "enable_thinking"];

let builtin: ModelConfigRules;
beforeAll(async () => {
  const release = JSON.parse(
    await readFile(
      new URL("../../../../../config/provider/zcode-builtin.json", import.meta.url),
      "utf8",
    ),
  );
  builtin = parseZCodeBuiltinModelConfigRules(release.config.modelConfigRules);
});

/** 模拟添加模型对话框（智能配置开）保存的个人规则：只有档位，没有 map；undefined = 未改档位。 */
function effectiveRules(modelId: string, values: string[] | undefined): ModelConfigRules {
  const config = values ? { optionSpecs: { reasoningLevel: { values } } } : {};
  return ModelConfigRules.composeEffective(
    builtin,
    parsePersonalModelConfigRules({
      providerModelRules: [{ providerId: "mistral", modelId, config }],
      manualProviderModelRules: [],
    }),
  );
}

async function captureReasoningFields(
  baseUrl: string,
  modelId: string,
  values: string[] | undefined,
  reasoningLevel: string,
): Promise<Record<string, unknown>> {
  const apiType = "openai-chat-completions";
  const config = effectiveRules(modelId, values).resolve({
    providerId: "mistral",
    modelId,
    apiType,
    baseUrl,
  });
  const modelConfig = createRegistryModelConfig(config);
  const provider = createRegistryProviderConfig(
    parseProviderConfig({
      group: "standard-personal",
      access: { type: "api-key", apiKey: "MISTRAL_FAKE_KEY" },
      api: { type: apiType, baseUrl },
    }),
  );
  if (!provider.ok || !modelConfig.ok) throw new Error("Invalid Mistral fixture");
  let body: Record<string, unknown> = {};
  const execution = new AiSdkModelExecution(
    { env: {} },
    {
      transport: async (input, init) => {
        body = await new Request(input, init).json();
        return Response.json(
          { error: { message: "MISTRAL_CAPTURED", type: "invalid_request_error" } },
          { status: 400 },
        );
      },
    },
  );
  const bound = execution.bindModel({
    providerId: "mistral",
    modelId,
    providerConfig: provider.config,
    supportsJsonSchemaOutput: false,
    optionSpecs: modelConfig.config.optionSpecs,
  });
  const resolved = {
    ...bound.resolveRequest({ options: { reasoningLevel, maxOutputTokens: 4096 } }),
    properties: modelConfig.config.properties,
  };
  const request: AiSdkModelTextRequest = { messages: [{ role: "user", content: "hi" }] };
  await expect(
    generateText(
      createGenerateTextOptions({
        includeModelIO: false,
        request,
        resolved,
        statusContext: createStatusContext({
          maxAttempts: 1,
          request,
          resolved,
          transport: "http",
        }),
      }),
    ),
  ).rejects.toThrow("MISTRAL_CAPTURED");
  return Object.fromEntries(REASONING_FIELDS.filter((k) => k in body).map((k) => [k, body[k]]));
}

describe("api.mistral.ai site reasoning rule", () => {
  it("defaults GLM models to low/high with high as the default (last) level", () => {
    // 实测（2026-11-08）：low ≈ 不思考、high 少量思考、max/省略为深度思考；用户不要 max。
    const config = effectiveRules("zai-glm-5-3", undefined).resolve({
      providerId: "mistral",
      modelId: "zai-glm-5-3",
      apiType: "openai-chat-completions",
      baseUrl: MISTRAL_URL,
    });
    expect(config.optionSpecs?.reasoningLevel?.values).toEqual(["low", "high"]);
  });

  it.each([
    ["zai-glm-5-3", undefined, "high", { reasoning_effort: "high" }],
    ["zai-glm-5-3", undefined, "low", { reasoning_effort: "low" }],
    ["zai-glm-5-3", ["high"], "high", { reasoning_effort: "high" }],
    // 旧对话框默认档位名：GLM 不能省略字段（省略 = 深度思考），disabled 改发 low。
    ["zai-glm-5-3-flash", ["disabled", "enabled"], "enabled", { reasoning_effort: "high" }],
    ["zai-glm-5-3-flash", ["disabled", "enabled"], "disabled", { reasoning_effort: "low" }],
    ["magistral-medium-latest", ["disabled", "enabled"], "disabled", {}],
    ["magistral-medium-latest", ["low", "high", "max"], "max", { reasoning_effort: "max" }],
  ])(
    "%s with values %j at %s sends only reasoning_effort",
    async (modelId, values, level, want) => {
      await expect(captureReasoningFields(MISTRAL_URL, modelId, values, level)).resolves.toEqual(
        want,
      );
    },
  );

  it("leaves the generic multi-field fallback for other hosts", async () => {
    // 对照：同样只有档位的个人规则，在非 Mistral 站点仍走通用兜底，证明差异来自站点规则。
    const fields = await captureReasoningFields(OTHER_URL, "zai-glm-5-3", ["high"], "high");
    expect(Object.keys(fields).sort()).toEqual([...REASONING_FIELDS].sort());
  });
});
