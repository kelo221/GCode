import {
  isApiKeyAccess,
  type ModelConfigObject,
  type ProviderConfigObject,
  type ProviderSettingsFacade,
  type ProviderSettingsProviderView,
  type ProviderSettingsView,
} from "@zcode/provider";
import {
  zcodeProviderSettingsMutateParamsSchema,
  zcodeProviderSettingsReadParamsSchema,
  zcodeProviderSettingsResolveModelConfigParamsSchema,
  type ZCodeProviderSettingsMutateResult,
  type ZCodeProviderSettingsResolveModelConfigResult,
  type ZCodeProviderSettingsOperation,
  type ZCodeProviderSettingsReadResult,
} from "@zcode/shared";
import {
  parseParams,
  ProtocolRequestError,
  type ZCodeProtocolAgentServerContext,
} from "./server-types.js";

const INVALID_PARAMS = -32602;
const NOT_CONFIGURED = -32020;
const CUSTOM_PROVIDER_GROUP = "standard-personal";

type UpdateProviderOperation = Extract<ZCodeProviderSettingsOperation, { type: "updateProvider" }>;

function requireFacade(context: ZCodeProtocolAgentServerContext): ProviderSettingsFacade {
  const facade = context.deps.providerSettings;
  if (!facade) {
    throw new ProtocolRequestError(NOT_CONFIGURED, "Provider settings runtime is not configured");
  }
  return facade;
}

export async function readProviderSettings(
  context: ZCodeProtocolAgentServerContext,
  rawParams: unknown,
): Promise<ZCodeProviderSettingsReadResult> {
  const params = parseParams(zcodeProviderSettingsReadParamsSchema, rawParams ?? {});
  const facade = requireFacade(context);
  const view = params.refresh ? await facade.refresh("gpui-settings") : facade.getView();
  return { view: toWire(view) };
}

export async function mutateProviderSettings(
  context: ZCodeProtocolAgentServerContext,
  rawParams: unknown,
): Promise<ZCodeProviderSettingsMutateResult> {
  const { operation } = parseParams(zcodeProviderSettingsMutateParamsSchema, rawParams);
  const facade = requireFacade(context);
  switch (operation.type) {
    case "createProvider": {
      const created = await facade.createPersonalProvider({
        ...(operation.templateId ? { templateId: operation.templateId } : {}),
        ...(operation.providerName ? { providerName: operation.providerName } : {}),
      });
      return { view: toWire(created.view), providerId: created.providerId };
    }
    case "updateProvider":
      return { view: toWire(await updateProvider(facade, operation)) };
    case "deleteProvider":
      requireCustomProvider(
        facade.getView(),
        operation.providerId,
        "Only custom providers can be deleted",
      );
      return { view: toWire(await facade.deletePersonalProvider(operation.providerId)) };
    case "reorderProviders":
      return { view: toWire(await facade.reorderPersonalProviders(operation.providerIds)) };
    case "addModel":
      // 与 Desktop 新增模型一致：未填写元数据时走推荐配置，未知模型由 issues 提示补全；
      // 弹窗提交的 config/useRecommendedConfig 原样交给 Facade，不在协议层重算 Overlay。
      return {
        view: toWire(
          await facade.addPersonalModel(
            operation.providerId,
            operation.modelId,
            (operation.config ?? {}) as ModelConfigObject,
            operation.useRecommendedConfig ?? true,
          ),
        ),
      };
    case "updateModel":
      // 编辑弹窗的唯一写入边界：改名、成员与精确 Rule 由 Facade 在同一次写入中提交；
      // basedOnRevision 过期时 Facade 抛出冲突，原样回到弹窗。
      return {
        view: toWire(
          await facade.savePersonalModelDraft({
            providerId: operation.providerId,
            originalModelId: operation.modelId,
            nextModelId: operation.nextModelId ?? operation.modelId,
            personalConfig: (operation.config ?? {}) as ModelConfigObject,
            ...(operation.useRecommendedConfig === undefined
              ? {}
              : { useRecommendedConfig: operation.useRecommendedConfig }),
            basedOnRevision: operation.basedOnRevision,
          }),
        ),
      };
    case "deleteModel": {
      const provider = requireProvider(facade.getView(), operation.providerId);
      if (provider.models.find((model) => model.modelId === operation.modelId)?.builtin) {
        throw new ProtocolRequestError(INVALID_PARAMS, "Built-in models cannot be deleted");
      }
      return {
        view: toWire(await facade.deletePersonalModel(operation.providerId, operation.modelId)),
      };
    }
    case "setModelEnabled":
      return {
        view: toWire(
          await facade.setPersonalModelEnabled(
            operation.providerId,
            operation.modelId,
            operation.enabled,
          ),
        ),
      };
    case "reorderModels":
      return {
        view: toWire(await facade.reorderPersonalModels(operation.providerId, operation.modelIds)),
      };
  }
}

export async function resolveProviderModelConfig(
  context: ZCodeProtocolAgentServerContext,
  rawParams: unknown,
): Promise<ZCodeProviderSettingsResolveModelConfigResult> {
  const params = parseParams(zcodeProviderSettingsResolveModelConfigParamsSchema, rawParams);
  const facade = requireFacade(context);
  requireProvider(facade.getView(), params.providerId);
  const resolution = facade.resolveModelConfig(
    params.personalConfig === undefined
      ? { providerId: params.providerId, modelId: params.modelId }
      : {
          providerId: params.providerId,
          originalModelId: params.modelId,
          modelId: params.modelId,
          personalConfig: params.personalConfig as ModelConfigObject,
        },
  );
  return {
    inheritedConfig:
      resolution.inheritedConfig as ZCodeProviderSettingsResolveModelConfigResult["inheritedConfig"],
    effectiveConfig:
      resolution.effectiveConfig as ZCodeProviderSettingsResolveModelConfigResult["effectiveConfig"],
    issues: [...resolution.issues],
  };
}

/**
 * 只写入用户改动的叶子字段，语义同 Desktop `ProviderDraftSave.ts`：重建整个 api/access
 * 会删除隐藏 headers，保存 Effective 对象又会把继承字段物化进 Personal Overlay。
 */
export function resolveProviderUpdate(
  provider: ProviderSettingsProviderView,
  operation: UpdateProviderOperation,
):
  | {
      readonly config: ProviderConfigObject;
      readonly metadata?: { providerName?: string; enabled?: boolean };
    }
  | undefined {
  const effective = provider.effectiveConfig;
  const custom = effective.group === CUSTOM_PROVIDER_GROUP;
  if (
    !custom &&
    (operation.providerName !== undefined ||
      operation.apiType !== undefined ||
      operation.baseUrl !== undefined)
  ) {
    throw new ProtocolRequestError(
      INVALID_PARAMS,
      "Preset provider name and endpoint are read-only",
    );
  }
  const access = effective.access;
  if (operation.apiKey !== undefined && !isApiKeyAccess(access)) {
    throw new ProtocolRequestError(INVALID_PARAMS, "This provider does not use an API key");
  }
  if (operation.enabled !== undefined && access?.type === "zhipu-account") {
    throw new ProtocolRequestError(INVALID_PARAMS, "Account providers cannot be toggled here");
  }

  const currentType = effective.api?.type ?? "anthropic-messages";
  const baseUrl =
    operation.baseUrl === undefined ? undefined : normalizeConfiguredBaseUrl(operation.baseUrl);
  const typeChanged = operation.apiType !== undefined && operation.apiType !== currentType;
  const urlChanged = baseUrl !== undefined && baseUrl !== (effective.api?.baseUrl ?? "");
  const keyChanged =
    operation.apiKey !== undefined &&
    isApiKeyAccess(access) &&
    operation.apiKey !== (access.apiKey ?? "");
  const nameChanged =
    operation.providerName !== undefined && operation.providerName !== provider.providerName;
  const enabledChanged = operation.enabled !== undefined && operation.enabled !== provider.enabled;
  if (!typeChanged && !urlChanged && !keyChanged && !nameChanged && !enabledChanged)
    return undefined;

  const {
    builtinModelIds: _builtinModelIds,
    personalModelIds: _personalModelIds,
    ...personal
  } = provider.personalConfig ?? {};
  const apiChanges = {
    ...(typeChanged || (urlChanged && !effective.api?.type)
      ? { type: operation.apiType ?? currentType }
      : {}),
    ...(urlChanged ? { baseUrl: baseUrl || undefined } : {}),
  };
  const config: ProviderConfigObject = {
    ...personal,
    ...(keyChanged && isApiKeyAccess(access)
      ? { access: { ...personal.access, type: access.type, apiKey: operation.apiKey } }
      : {}),
    ...(typeChanged || urlChanged ? { api: { ...personal.api, ...apiChanges } } : {}),
  } as ProviderConfigObject;
  const metadata = {
    ...(nameChanged ? { providerName: operation.providerName } : {}),
    ...(enabledChanged ? { enabled: operation.enabled } : {}),
  };
  return { config, ...(nameChanged || enabledChanged ? { metadata } : {}) };
}

async function updateProvider(
  facade: ProviderSettingsFacade,
  operation: UpdateProviderOperation,
): Promise<ProviderSettingsView> {
  const view = facade.getView();
  const update = resolveProviderUpdate(requireProvider(view, operation.providerId), operation);
  if (!update) return view;
  return facade.savePersonalProviderOverlay(operation.providerId, update.config, update.metadata);
}

function requireProvider(
  view: ProviderSettingsView,
  providerId: string,
): ProviderSettingsProviderView {
  const provider = view.providers.find((item) => item.providerId === providerId);
  if (!provider) throw new ProtocolRequestError(INVALID_PARAMS, `Unknown provider: ${providerId}`);
  return provider;
}

function requireCustomProvider(view: ProviderSettingsView, providerId: string, message: string) {
  if (requireProvider(view, providerId).effectiveConfig.group !== CUSTOM_PROVIDER_GROUP) {
    throw new ProtocolRequestError(INVALID_PARAMS, message);
  }
}

/** 与 Desktop 表单同一规则：去尾斜杠，并修复粘贴时重复的协议+主机前缀。 */
function normalizeConfiguredBaseUrl(value: string): string {
  const normalized = value.trim().replace(/\/+$/, "");
  if (!normalized) return "";
  try {
    const parsed = new URL(normalized);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return normalized;
    const marker = `${parsed.protocol}//${parsed.host}`;
    const duplicateIndex = normalized.indexOf(marker, marker.length);
    if (duplicateIndex < 0) return normalized;
    const firstUrl = normalized.slice(0, duplicateIndex).replace(/\/+$/, "");
    const secondUrl = normalized.slice(duplicateIndex).replace(/\/+$/, "");
    return firstUrl === secondUrl ? firstUrl : normalized;
  } catch {
    return normalized;
  }
}

function toWire(view: ProviderSettingsView): ZCodeProviderSettingsReadResult["view"] {
  return {
    revision: view.revision,
    providerTemplates: [...view.providerTemplates],
    providerOrder: [...view.providerOrder],
    providers: [...view.providers],
  };
}
