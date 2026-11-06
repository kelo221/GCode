/* eslint-disable max-lines -- subagent runtime wiring 集中衔接 child runtime、tool pool、权限、MCP 与 activity watchdog，拆分需单独迁移。 */
import {
  COMPACT_TOOL_NAME,
  FIND_FILES_TOOL_NAME,
  RESPOND_TO_COORDINATOR_TOOL_NAME,
  SEARCH_CODE_TOOL_NAME,
  SESSION_ENTRY_MODEL_SELECTION,
  parseModelSelectionValue,
} from "@zcode/contracts";
import type { SubagentRunOptions } from "@zcode/contracts";
import { inheritPermissionMode } from "@zcode/contracts";
import {
  defaultScheduler,
  PermissionService,
  defaultPermissionConfig,
  buildExploreAllowedTools,
  buildExploreAgentPrompt,
  createExploreSubagentPort,
  createCoreError,
  CoreErrorType,
} from "../deps.js";
import type {
  CollaborationMode,
  ExploreSubagentRuntimeRequest,
  McpConnectionSnapshot,
  McpPort,
  Model,
  ModelSelection,
  SkillContent,
  SkillLoadOutcome,
  SkillOperationOptions,
  SkillPort,
  SubagentPort,
} from "../deps.js";
import { AgentRuntime } from "../agent-runtime.js";
import type { AgentRuntimeInternal } from "../internal.js";
import { applyRuntimeExecutionState, onRuntimeExecutionStateChanged } from "../execution-state.js";
import { cloneModelSelection } from "../model-selection.js";
import { resolveSubagentSelection } from "../helpers/subagent-selection.js";
import type { AgentRuntimeDeps } from "../types.js";
import { toMcpToolName } from "../../mcp/index.js";
import { createBorrowedSubagentMcpAccess } from "../../subagent/borrowed-mcp-port.js";
import { createSubagentMessageSink } from "../../subagent/message-steering.js";
import { latestAnnouncedEnvironment } from "./context-refresh.js";
import {
  extractRequiredMcpServerNames,
  matchesRequiredMcpServer,
} from "../../subagent/mcp-config.js";
import {
  isMirroredSubagentToolEvent,
  mirrorSubagentToolEvent,
} from "../../subagent/tool-event-mirror.js";
import { isBuiltInExploreAgentProfile } from "../../subagent/profile.js";
import {
  buildSubagentChildDisallowRules,
  filterSubagentChildToolNames,
} from "../../subagent/tool-policy.js";
import { withCodeSearchEquivalents } from "../../subagent/explore-tools.js";
import { isSubagentDispatchToolName } from "../../tool/compat.js";
import { resolveEmbeddedSearchBranchCapability } from "../../embedded-search/capability.js";
import { getSessionShellEnvironment } from "./session-shell-environment.js";
import { deriveChildClientPorts } from "../helpers/child-client-ports.js";
import { createCoordinatorResponsePort } from "../../subagent/coordinator-response.js";
import { isStaleBranchRuntimeTaskEvent } from "./runtime-command-generation.js";
import { loadPersistentAgentMemory } from "../../subagent/persistent-memory.js";
import { withExecutionEnv } from "../../subagent/worktree-isolation.js";
import {
  createOfficialCuaPolicy,
  SUBAGENT_COMPUTER_USE_UNAVAILABLE_CODE,
  SUBAGENT_COMPUTER_USE_UNAVAILABLE_MESSAGE,
  type OfficialCuaPolicy,
} from "../../subagent/computer-use-policy.js";
import { computeOfficialCuaServerNames } from "./mcp.js";
import { persistRuntimeModelSelection, sameModelSelection } from "./turn-model.js";
import { getOffPeakExecution, markTemporaryRuntimeSelection } from "../off-peak/runtime.js";
import { getOffPeakModelBinding } from "../off-peak/model-binding.js";

export function createDefaultSubagentPort(
  this: AgentRuntimeInternal,
  deps: AgentRuntimeDeps,
): SubagentPort | undefined {
  if (this.config.subagents?.enabled === false) {
    return undefined;
  }
  const nested = this.config.taskType === "subagent_child";

  const ownedRuntimes = new Set<AgentRuntime>();
  return createExploreSubagentPort({
    closeOwnedRuntimes: async () => {
      const results = await Promise.allSettled(
        [...ownedRuntimes].map(async (runtime) => {
          await runtime.close();
          ownedRuntimes.delete(runtime);
        }),
      );
      const errors = results.flatMap((result) =>
        result.status === "rejected" ? [result.reason] : [],
      );
      if (errors.length) throw new AggregateError(errors, "Subagent runtimes cleanup failed");
      ownedRuntimes.clear();
    },
    logger: this.logger,
    inactivityTimeoutMs: this.config.subagents?.inactivityTimeoutMs,
    autoBackgroundMs: nested ? undefined : this.config.subagents?.autoBackgroundMs,
    foregroundOnly: nested,
    outputRootDir: this.config.subagents?.outputRootDir,
    getAgentDefinitions: this.getAgentDefinitions,
    builtInModelSelectionOverrides: this.config.subagents?.builtInModelSelectionOverrides,
    runtimeTaskRegistry: this.runtimeTaskRegistry,
    worktreeIsolation: deps.worktreeIsolationPort,
    emitParentEvent: async (event, traceContext) => {
      if (isStaleBranchRuntimeTaskEvent(this, event)) return;
      await this.appendEvent(event, traceContext);
    },
    enqueueParentTaskNotification: (notification) => {
      this.enqueueBackgroundTaskNotification({
        originMeta: notification.originMeta,
        taskId: notification.taskId,
        text: notification.text,
        traceContext: notification.traceContext,
      });
      return undefined;
    },
    getAllowedTools: () => {
      return buildExploreAllowedTools({
        embeddedSearchEnabled: resolveSubagentEmbeddedSearchEnabled(),
        codeSearch: deps.codeSearchPort !== undefined,
      });
    },
    runExploreAgent: async (request, options) => {
      const offPeak = getOffPeakExecution(this);
      const childBinding = offPeak?.createChildBinding();
      if (offPeak && !offPeak.lifecycle.terminal) {
        const selection = offPeak.lifecycle.ownedByUser
          ? this.getSessionModelSelection()
          : offPeak.selection;
        if (selection)
          options = {
            ...options,
            modelOverride: {
              selection,
              requestDependencies: childBinding?.requestDependencies,
              background: "allow",
            },
          };
      }
      try {
        request.reportActivity?.();
        const builtInExplore = isBuiltInExploreAgentProfile(request.profile);
        const agentsMdInstructions =
          request.profile.injectAgentsMd !== false
            ? this.contextSourceSnapshot?.userInstructions
            : undefined;
        const { selection: profileChildSelection, hasConcreteModel } = resolveSubagentSelection({
          profileSelection: request.profile.modelSelection,
          parentSelection: this.getSessionModelSelection(),
          overrideSelection: options?.modelOverride?.selection,
          resolveSelection: deps.resolveEffectiveModelSelection,
        });
        const modelOverride = options?.modelOverride;
        const inheritedModel = !modelOverride && !hasConcreteModel ? options?.model : undefined;
        // Core Server override 优先于持久化 profile 与父模型继承，但仍只是标准 Selection。
        const childSelection = inheritedModel
          ? modelSelectionFromActiveModel(inheritedModel)
          : profileChildSelection;
        const embeddedSearchEnabled = resolveSubagentEmbeddedSearchEnabled();
        // worktree 隔离：执行目录与按根绑定的服务都换到 worktree；session 关联、Memory
        // 与 skill/AGENTS.md 仍沿用父工作区。没有 scope 工厂时不继承父的 code search 索引。
        const isolation = request.isolation;
        const executionDirectory = isolation?.executionDirectory ?? request.workingDirectory;
        const scopedPorts = isolation
          ? deps.scopePortsFor?.({
              root: executionDirectory,
              ...(isolation.env ? { env: isolation.env } : {}),
            })
          : undefined;
        const childExecutionPort = isolation
          ? withExecutionEnv(deps.executionPort, isolation.env)
          : deps.executionPort;
        const childCodeSearchPort = isolation ? scopedPorts?.codeSearchPort : deps.codeSearchPort;
        const childDiagnosticsPort = isolation
          ? (scopedPorts?.diagnosticsPort ?? deps.diagnosticsPort)
          : deps.diagnosticsPort;
        const baseChildEnvInfo = this.contextSourceSnapshot?.envInfo ??
          this.config.envInfo ?? {
            cwd: executionDirectory,
            platform: "unknown",
            shell: "unknown",
            osVersion: "unknown",
            nodeVersion: "unknown",
          };
        const shellEnvironment = getSessionShellEnvironment(this);
        const bashShellSelection = shellEnvironment?.selection;
        const childEnvInfo = {
          ...baseChildEnvInfo,
          ...(shellEnvironment ? { shell: shellEnvironment.promptShell } : {}),
        };
        const baseAgentPrompt =
          builtInExplore && request.systemPrompt?.trim() === ""
            ? buildExploreAgentPrompt({
                embeddedSearchEnabled,
                // 与子 runtime 注册 FindFiles/SearchCode 的门一致（includeCodeSearch 看的就是这个 port）。
                codeSearchEnabled: childCodeSearchPort !== undefined,
              })
            : request.systemPrompt?.trim();
        const persistentMemory = await loadPersistentAgentMemory({
          fileSystemPort: deps.fileSystemPort,
          logger: this.logger,
          memory: this.config.memory,
          profile: request.profile,
          traceContext: request.traceContext,
          workspaceRoot: request.workspaceRoot,
        });
        // 空 agent prompt 不是一个语义段；先硬拼 `\n\n` 会把缺失段的边界
        // 泄漏到 persistent Memory 开头。这里只组合非空正文，block 左边界由 builder 统一添加。
        const agentPrompt = [baseAgentPrompt, persistentMemory?.prompt]
          .filter((part): part is string => typeof part === "string" && part.length > 0)
          .join("\n\n");
        const announcedEnvironment = latestAnnouncedEnvironment(
          this.messageHistory.borrowReadOnlyRuntimeEntries(),
        );
        const childRuntimeEnvInfo = {
          ...childEnvInfo,
          // 父会话切目录后，起始快照的仓库属性已过期；只继承同一 cwd 的最近已接受环境。
          // 附加目录属于父运行配置，child 不继承，不能随快照混入 envInfo。
          ...(announcedEnvironment?.cwd === executionDirectory
            ? {
                platform: announcedEnvironment.platform,
                shell: announcedEnvironment.shell,
                osVersion: announcedEnvironment.osVersion,
                isGitRepository: announcedEnvironment.isGitRepository,
              }
            : isolation
              ? {
                  platform: announcedEnvironment?.platform ?? childEnvInfo.platform,
                  shell: announcedEnvironment?.shell ?? childEnvInfo.shell,
                  osVersion: announcedEnvironment?.osVersion ?? childEnvInfo.osVersion,
                  isGitRepository: true,
                }
              : {}),
          cwd: executionDirectory,
        };
        const officialCuaServerNames = computeOfficialCuaServerNames(
          this.config.mcp?.servers ?? {},
          new Set(this.config.mcp?.trustedOfficialCuaServerNames ?? []),
        );
        const preflightCuaPolicy = createOfficialCuaPolicy(
          officialCuaServerNames,
          [],
          this.config.pluginReferenceCatalog,
        );
        await validateSubagentComputerUseConfiguration(request, preflightCuaPolicy, this.skillPort);
        const childMcpAccess = await resolveSubagentMcpAccess.call(
          this,
          request,
          officialCuaServerNames,
        );
        const childToolAllowlist = resolveSubagentToolAllowlist.call(
          this,
          request,
          childMcpAccess.snapshot?.tools.map((descriptor) => toMcpToolName(descriptor)) ?? [],
        );
        validateSubagentMcpRequirements(request, childToolAllowlist, childMcpAccess);
        // child 执行状态只由父 runtime 当前状态派生；spawn、resume 与父切换共用这一处。
        const deriveChildExecutionState = (): { mode: CollaborationMode; planEnabled: boolean } => {
          const childMode = resolveSubagentPermissionMode(
            this.getPlanEnabled() ? "plan" : this.config.mode,
            request.permissionMode,
            builtInExplore,
          );
          // 旧 plan 枚举不包含基础权限；拆分后继承完整状态，避免被构造器回退成 build。
          return childMode === "plan"
            ? { mode: this.config.mode ?? "build", planEnabled: true }
            : { mode: childMode ?? "build", planEnabled: false };
        };
        const childExecutionState = deriveChildExecutionState();
        const childCuaPolicy = createOfficialCuaPolicy(
          officialCuaServerNames,
          childMcpAccess.parentSnapshot?.tools ?? childMcpAccess.snapshot?.tools ?? [],
          this.config.pluginReferenceCatalog,
        );
        await validateSubagentComputerUseConfiguration(request, childCuaPolicy, this.skillPort);
        const childSkillPort = resolveSubagentSkillPort(
          this.skillPort,
          request.profile.skills,
          childCuaPolicy,
        );
        const baseChildModelFactory = modelOverride
          ? createSubagentOverrideModelFactory(modelOverride, this.modelFactory)
          : inheritedModel
            ? createInheritedSubagentModelFactory(childSelection, inheritedModel, this.modelFactory)
            : this.modelFactory;
        if (!baseChildModelFactory) {
          throw createCoreError(
            CoreErrorType.ConfigurationError,
            `Subagent model factory cannot resolve ${childSelection.providerId}/${childSelection.modelId}`,
            { recoverable: true },
          );
        }
        const childModel = baseChildModelFactory({ selection: childSelection });
        const childModelFactory: NonNullable<AgentRuntimeDeps["modelFactory"]> = (target) =>
          target.selection.providerId === childSelection.providerId &&
          target.selection.modelId === childSelection.modelId &&
          target.selection.options?.reasoningLevel === childSelection.options?.reasoningLevel
            ? childModel
            : baseChildModelFactory(target);
        const parentToolCallId = traceStringAttribute(request.traceContext, "parentToolCallId");
        // 对外交互端口（permission broker + provider runtime headers）：与 dwf actor、legacy
        // workflow child 共用同一条派生，路由身份统一落到本 runtime 的会话。
        const childClientPorts = deriveChildClientPorts(
          {
            permissionBroker: this.permissionBroker,
            ...(this.providerRuntimeHeadersPort === undefined
              ? {}
              : { providerRuntimeHeadersPort: this.providerRuntimeHeadersPort }),
          },
          {
            agentId: request.agentId,
            agentType: request.agentType,
            childSessionId: request.sessionId,
            description: request.description,
            parentSessionId: this.sessionId,
            parentToolCallId,
            ...(request.traceContext.turnId === undefined
              ? {}
              : { parentTurnId: request.traceContext.turnId }),
          },
        );
        // Browser Use：子代理与当前对话共用 tab（tabOwner: "parent"），用户能在面板里看到它开的 tab，
        // 它也能看到对话里已有的 tab；workspace / clientMode 经 forChildSession 登记回父会话解析。
        // 子代理结束只撤销登记，不关 tab。端口没有 forChildSession（CLI headless）时退回子会话自己的 scope。
        const childBrowserControlPort =
          this.browserControlPort?.forChildSession?.({
            childSessionId: request.sessionId,
            parentSessionId: this.sessionId,
            tabOwner: "parent",
          }) ?? this.browserControlPort;
        const mirroredToolNameByChildToolCallId = new Map<string, string>();
        let sessionReadyNotified = false;
        const notifySessionReady = async () => {
          if (sessionReadyNotified) return;
          await request.onSessionReady?.({
            model: `${childSelection.providerId}/${childSelection.modelId}`,
          });
          sessionReadyNotified = true;
        };
        this.logger?.debug("Starting subagent child runtime", {
          parentSessionId: this.sessionId,
          childSessionId: request.sessionId,
          agentType: request.agentType,
        });
        const childRuntime = new AgentRuntime(
          request.sessionId,
          {
            mode: childExecutionState.mode,
            planEnabled: childExecutionState.planEnabled,
            // 模型选择的影响不只在最终 request.model：MCS、内建搜索与 token/media 预算会在
            // child runtime 内按 default model 预先塑形。同步 child 因此必须把整套执行
            // 配置都指向父 turn 快照；runner 禁止它转后台，provider registry 则由父 turn
            // finally 清理，快照不会成为可恢复的 session 配置。
            modelSelection: cloneModelSelection(childSelection),
            modelContextBudgetStrategy: this.config.modelContextBudgetStrategy,
            workingDirectory: executionDirectory,
            ...(isolation
              ? {
                  workspacePath: request.workingDirectory,
                  executionDirectoryOverride: executionDirectory,
                }
              : {}),
            // 执行模型只由 child Active Model 投影进 Context；envInfo 不保存第二份模型事实。
            envInfo: childRuntimeEnvInfo,
            // Explore 子运行时之前没有继承主会话的流式配置，Protocol 桌面端虽已默认
            // 开启 modelStreaming，子请求仍会退回 generateText。部分 OpenAI-compatible 端点在
            // 非流式请求里也返回 SSE `data:` 帧，generateText 会按普通 JSON 解析并报
            // Invalid JSON response；继承父配置可让子 agent 与主链路走同一 streamText 语义。
            modelStreaming: this.config.modelStreaming,
            bashTimeoutPolicy: this.config.bashTimeoutPolicy,
            midConversationSystem: this.config.midConversationSystem,
            toolSearch: this.config.toolSearch,
            bashShellSelection,
            // child 只复用父 runtime 已解析的 instructions snapshot；Project Context 仍不继承。
            currentDate: this.contextSourceSnapshot?.currentDate ?? this.config.currentDate,
            subagentContext: {
              agentPrompt: agentPrompt ?? "",
              taskBrief: request.prompt,
              ...(agentsMdInstructions ? { userInstructions: agentsMdInstructions } : {}),
            },
            agentName: `zcode-${request.agentType}`,
            maxTurns: request.maxTurns ?? this.config.subagents?.maxTurns ?? 4,
            parentSessionId: this.sessionId,
            taskType: "subagent_child",
            // 动态工作流灰度门必须结构性继承：
            // 父会话关着而子代理开着，等于 Agent 工具变成绕过灰度的后门。默认路径（child 继承
            // 父 registry 可见的工具名）本来就够，但**自定义 agent profile 显式写
            // `allowedTools: ["CreateWorkflow"]` 时会跳过那次交集**，只剩这一道能挡住。
            dynamicWorkflowEnabled: this.config.dynamicWorkflowEnabled,
            // launch.md「On demand: activation」Children 段：子代理继承父会话**此刻**的工具面——
            // 父未激活则子也按需（子会话里没有 `/workflow` 入口，实际等于没有工作流工具）；
            // 父已激活则子出生即注册。
            dynamicWorkflowToolsOnDemand:
              this.config.dynamicWorkflowToolsOnDemand === true &&
              !this.dynamicWorkflowToolsActivated,
            // 修复原因：默认 subagent 已从 Explore 调整为 general-purpose。
            // toolset 不能再依赖 DEFAULT_SUBAGENT_TYPE，否则默认通用 agent 会被误降级为只读搜索工具面。
            toolset: builtInExplore ? "explore" : "main",
            toolAllowlist: childToolAllowlist,
            toolDisallowlist: this.config.toolDisallowlist,
            embeddedSearchBackend: this.config.embeddedSearchBackend,
            nativeSearchEnhancementsEnabled: this.config.nativeSearchEnhancementsEnabled,
            // 非 Explore 的 subagent 可以派发前台 Explore（见 getAgentDefinitions 与 runner
            // foregroundOnly）；Explore 自己不再派发，深度固定两层。
            subagents: {
              backgroundBashMaxMs: this.config.subagents?.backgroundBashMaxMs,
              enabled: !builtInExplore && this.config.taskType !== "subagent_child",
              inactivityTimeoutMs: this.config.subagents?.inactivityTimeoutMs,
              outputRootDir: this.config.subagents?.outputRootDir,
            },
            mcp: childMcpAccess.config,
          },
          {
            agentTelemetry: this.agentTelemetry.port,
            agentTelemetryCausation: this.agentTelemetry.captureCausation(),
            // 前台 child 的生命周期被父 Agent Tool await，使用真实父子 Span；后台 child
            // 可能晚于父 Tool/Turn 结束，只能作为独立 Trace 用 Link 保留因果关系。
            agentTelemetryCausationMode: request.background ? "linked_root" : "child",
            eventStore: this.eventStore,
            // 日期公告取运行时时钟；child 继承父时钟，父子只有一个日期来源。
            now: this.now,
            sessionStore: deps.sessionStore,
            // 共享宿主环境读取端口，只用于 child 内 cd 之后的环境探测；初始快照和指令都取父级传下的配置。
            contextSourcePort: this.contextSourcePort,
            // 子 runtime 的模型请求准入：装配方给了 subagent 工厂就用它（受闸门约束，同一父会话的
            // subagent 共用一条队列）；没给则沿用父的端口。
            // 修复原因：subagent 曾直接继承父的 observer（不排队、不看 Retry-After），一轮并行多个
            // Agent 调用会放大成 1302 风暴（docs/dynamic-workflow/concurrency.md「Where the port is bound」）。
            modelRequestAdmission:
              deps.subagentModelRequestAdmission?.(this.sessionId) ?? this.modelRequestAdmission,
            modelFactory: childModelFactory,
            resolveEffectiveModelSelection: deps.resolveEffectiveModelSelection,
            // 子 runtime 自己仍使用 request.sessionId 做事件持久化和 trace 归档；对外阻塞交互
            // （permission / AskUserQuestion / provider runtime headers）一律路由回父 session——
            // 桌面 UI 只认识父 task 的 sessionId。派生收敛在 deriveChildClientPorts 一处，
            // dwf actor 与 legacy workflow child 走同一条。
            ...childClientPorts,
            ...(childBrowserControlPort === undefined
              ? {}
              : { browserControlPort: childBrowserControlPort }),
            coordinatorResponsePort: createCoordinatorResponsePort({
              agentId: request.agentId,
              agentType: request.agentType,
              childSessionId: request.sessionId,
              parentToolCallId,
              enqueue: (input) => this.enqueueSubagentMessage(input),
            }),
            // Explore 使用独立只读权限配置；general-purpose 和自定义 agent 继承父权限服务。
            permissionService: builtInExplore
              ? new PermissionService(defaultPermissionConfig)
              : this.permissionService,
            toolScheduler: deps.toolScheduler ?? defaultScheduler,
            executionPort: childExecutionPort,
            fileSystemPort: deps.fileSystemPort,
            codeSearchPort: childCodeSearchPort,
            diagnosticsPort: childDiagnosticsPort,
            // Explore 子运行时会暴露 WebFetch，但之前没有继承主 runtime 的
            // HTTP client port，导致工具在真正发请求前抛出配置错误，而不是网络请求失败。
            httpClientPort: deps.httpClientPort,
            imageProcessorPort: deps.imageProcessorPort,
            pdfDocumentPort: deps.pdfDocumentPort,
            memoryRoot: persistentMemory?.rootDir,
            mcpPort: childMcpAccess.port,
            skillPort: childSkillPort,
            // 子 runtime 的 Agent 列表与 launch 解析都只看到内置 Explore（只读、不再派发）。
            getAgentDefinitions: () => ({
              activeAgents: this.getAgentDefinitions().activeAgents.filter(
                isBuiltInExploreAgentProfile,
              ),
            }),
            artifactStore: deps.artifactStore,
            appVersion: this.appVersion,
            eventSink: {
              onSessionEvent: async (event) => {
                request.reportActivity?.();
                // child runtime 的事件已经按 childSessionId 落库，但旧链路只把
                // 少量工具事件镜像给 parent sink，导致 UI 订阅 child topic 后只能拿到打开时
                // 的 hydration，后续流式内容不会更新。raw child event 只通知父 runtime 的
                // 外部 sinks，不再次 append，因此不会重复持久化；bootstrap 再按 event.sessionId
                // 把它路由到 child publisher。
                await this.notifyEventSinks(event, {
                  ...request.traceContext,
                  sessionId: request.sessionId,
                });
                // 孙 agent 的原始事件（sessionId 是孙会话）和 child 镜像出的孙工具活动只属于
                // child transcript；再镜像会让主会话 timeline 出现孙 agent 的工具调用。
                // 权限交互的镜像不带 source，仍逐层上送到主会话。
                if (event.sessionId !== request.sessionId || isMirroredSubagentToolEvent(event)) {
                  return;
                }
                const mirroredEvent = mirrorSubagentToolEvent(event, {
                  agentId: request.agentId,
                  agentType: request.agentType,
                  background: request.background,
                  childSessionId: request.sessionId,
                  description: request.description,
                  parentSessionId: this.sessionId,
                  parentToolCallId,
                  parentTurnId: request.traceContext.turnId,
                  toolNameByChildToolCallId: mirroredToolNameByChildToolCallId,
                });
                if (!mirroredEvent) return;

                // parent mirror 保留原语义：父会话只看到 subagent 摘要/工具活动，raw child
                // 正文不会污染父 timeline。
                await this.notifyEventSinks(mirroredEvent, {
                  ...request.traceContext,
                  sessionId: this.sessionId,
                });
              },
            },
            logger: this.logger,
            traceContext: request.traceContext,
          },
        );

        // 已完成的 child 仍可能拥有后台 Bash、title/MCP 等工作；父 session 关闭时仍需找到它。
        ownedRuntimes.add(childRuntime);
        const childInternal = childRuntime as unknown as AgentRuntimeInternal;
        const followChildExecutionState = () =>
          applyRuntimeExecutionState(childInternal, deriveChildExecutionState(), {
            source: "command",
            traceContext: request.traceContext,
          });
        // 父会话切换模式（如运行中改为「完全访问」）同步给仍在运行的 child。
        const stopFollowingParent = onRuntimeExecutionStateChanged(this, async () => {
          await followChildExecutionState();
        });
        try {
          const resumesExistingChild = request.resumeFromStore === true;
          if (childBinding)
            markTemporaryRuntimeSelection(
              childRuntime as unknown as AgentRuntimeInternal,
              offPeak?.ordinarySelection,
            );
          let previousSelection: ModelSelection | undefined;
          if (resumesExistingChild) {
            const persistedMessages = await this.sessionStore?.messages({
              sessionID: request.sessionId,
            });
            // terminal resume 只恢复已有历史；模型初始化前失败不能变成一次空 session 创建。
            if (!persistedMessages?.length) {
              throw createCoreError(
                CoreErrorType.SessionNotFound,
                `No transcript found for agent ${request.agentId}`,
                { recoverable: true },
              );
            }
            const entries = await this.sessionStore?.sessionEntries?.({
              sessionID: request.sessionId,
              type: SESSION_ENTRY_MODEL_SELECTION,
            });
            previousSelection = parseModelSelectionValue(entries?.at(-1)?.data);
            await childRuntime.resumeFromStore({
              persistedMessages,
              traceContext: request.traceContext,
            });
            // 修复原因：resumeFromStore 恢复的是 child 自己持久化的模式（常为 spawn 时的 build），
            // 「继续」已停止的子代理会忽略父会话当前的「完全访问」。恢复后按父状态重新派生。
            await followChildExecutionState();
            // 恢复只导入历史；本轮 profile 选出的模型仍是执行权威，并更新冷恢复所用的选择记录。
            await persistRuntimeModelSelection(
              childRuntime as unknown as AgentRuntimeInternal,
              childSelection,
            );
          } else {
            // 父会话过去先发布 SubagentSpawned，child 的首轮 executeTurn 才落库。
            // 并发派生时目录查询会在两者之间读到少一个 child。这里把持久化提升为发布前闸门。
            await childRuntime.ensureSessionPersistedForExternalActivity(request.prompt, {
              traceContext: request.traceContext,
            });
          }
          await notifySessionReady();
          if (
            !resumesExistingChild ||
            (previousSelection && !sameModelSelection(previousSelection, childSelection))
          ) {
            // 同模型恢复不重复插入分隔线；配置变化复用现有事件和 transcript 记录。
            childRuntime.recordPendingModelChange({
              ...(previousSelection
                ? {
                    fromModel: previousSelection,
                    fromModelLabel: `${previousSelection.providerId}/${previousSelection.modelId}`,
                  }
                : {}),
              toModel: childSelection,
              toModelLabel: `${childSelection.providerId}/${childSelection.modelId}`,
            });
            await childRuntime.emitModelSelected({
              modelSelection: childSelection,
              effectiveReasoningLevel: childModel.options.reasoningLevel,
              previousModelSelection: previousSelection ?? null,
              traceContext: request.traceContext,
            });
          }
          request.registerMessageSink?.(createSubagentMessageSink(childRuntime, request));
          const turnResult = await childRuntime.executeTurn(request.prompt, undefined, {
            ...(childBinding
              ? {
                  offPeakTaskId: offPeak!.offPeakTaskId,
                  modelExecution: {
                    selectionScope: "execution" as const,
                    memoryExtraction: "skip" as const,
                  },
                }
              : { offPeakTaskId: undefined }),
            abortSignal: options?.signal,
            // 子 Runtime 的首轮输入来自父 Agent，而不是真实用户直接输入；保留源事实，避免
            // Subagent Turn 在 Trace 和成功率报表里被误归类为 user。
            inputSource: "subagent",
            inputPresentation: "coordinator_input",
            traceContext: request.traceContext,
          });
          return { ...turnResult, contextWindow: childModel.properties.contextWindow };
        } finally {
          stopFollowingParent();
          const cancelled = options?.signal?.aborted === true;
          childRuntime.sealBackgroundTaskNotifications({
            reason: cancelled ? "subagent_cancelled" : "subagent_terminal",
            traceContext: request.traceContext,
          });
          if (cancelled || isolation) {
            // worktree 收尾（提交、删除）前必须停掉 child 留下的后台写入者，数量随结果报告。
            const stopped = await childRuntime.cancelRunningRuntimeBackgroundTasks({
              reason: cancelled ? "subagent_cancelled" : "subagent_worktree_finalize",
              traceContext: request.traceContext,
            });
            isolation?.onBackgroundTasksStopped(stopped);
          }
          await childBrowserControlPort
            ?.closeSession?.({ sessionId: request.sessionId, traceContext: request.traceContext })
            .catch((error: unknown) => {
              this.logger?.warn("Subagent browser session cleanup failed", {
                error: error instanceof Error ? error.message : String(error),
                event: "browser.session_cleanup.failed",
              });
            });
          // 执行结果与资源释放分开；失败保留句柄，最终 owner close 依据当时释放结果判断。
          const released = cancelled ? childRuntime.close() : childRuntime.retire();
          const release = released.then(
            () => {
              ownedRuntimes.delete(childRuntime);
            },
            (error: unknown) =>
              this.logger?.warn("Subagent resource cleanup pending", {
                event: "subagent.cleanup.pending",
                childSessionId: request.sessionId,
                error: error instanceof Error ? error.message : String(error),
              }),
          );
          if (cancelled || !childRuntime.hasResidencyBlockingWork()) await release;
        }
      } finally {
        childBinding?.release();
      }
    },
  });
}

function resolveSubagentEmbeddedSearchEnabled(): boolean {
  const embeddedSearchDecision = resolveEmbeddedSearchBranchCapability({
    bashAvailable: true,
  });

  return embeddedSearchDecision.useEmbeddedSearchBranch;
}

export function createInheritedSubagentModelFactory(
  inheritedSelection: ModelSelection,
  inheritedModel: Model,
  fallbackFactory: AgentRuntimeDeps["modelFactory"],
): NonNullable<AgentRuntimeDeps["modelFactory"]> {
  return (target) => {
    if (
      target.selection.providerId === inheritedSelection.providerId &&
      target.selection.modelId === inheritedSelection.modelId &&
      target.selection.options?.reasoningLevel === inheritedSelection.options?.reasoningLevel
    ) {
      // Selection 保持显式意图的稀疏形态；不能拿它和 Active Model 的完整 effective
      // options 比较，否则默认值必然导致复用失败并让 child 重新解释可变 Registry。
      return inheritedModel;
    }
    return fallbackFactory(target);
  };
}

export function modelSelectionFromActiveModel(model: Model): ModelSelection {
  const reasoningLevel = model.options.reasoningLevel;
  return {
    providerId: model.providerId,
    modelId: model.modelId,
    ...(reasoningLevel ? { options: { reasoningLevel } } : {}),
  };
}

function createSubagentOverrideModelFactory(
  override: NonNullable<SubagentRunOptions["modelOverride"]>,
  fallbackFactory: AgentRuntimeDeps["modelFactory"],
): AgentRuntimeDeps["modelFactory"] {
  return (target) => {
    const model = fallbackFactory({
      ...target,
      selection: override.selection,
      requestDependencies: override.requestDependencies,
    });
    return getOffPeakModelBinding(override.requestDependencies)?.wrap(model) ?? model;
  };
}

function resolveSubagentPermissionMode(
  parentMode: AgentRuntimeInternal["config"]["mode"],
  permissionMode: ExploreSubagentRuntimeRequest["permissionMode"],
  builtInExplore: boolean,
): AgentRuntimeInternal["config"]["mode"] {
  switch (permissionMode) {
    case "bypassPermissions":
    case "dontAsk":
      return inheritPermissionMode(parentMode, "yolo");
    case "acceptEdits":
      return "edit";
    case "auto":
      return "auto";
    case "plan":
      return "plan";
    case "default":
    case undefined:
      return inheritPermissionMode(parentMode, builtInExplore ? "yolo" : parentMode);
    default:
      return parentMode;
  }
}

function resolveSubagentToolAllowlist(
  this: AgentRuntimeInternal,
  request: ExploreSubagentRuntimeRequest,
  visibleMcpToolNames: readonly string[],
): readonly string[] {
  const disallowedRules = buildSubagentChildDisallowRules([
    ...(this.config.toolDisallowlist ?? []),
    ...(request.disallowedTools ?? []),
  ]);
  const inheritsAvailableTools =
    request.allowedTools.length === 0 || request.allowedTools.includes("*");
  if (inheritsAvailableTools) {
    const parentAllowedMcpToolNames = filterMcpToolNamesByParentAllowlist(
      visibleMcpToolNames,
      this.config.toolAllowlist,
    );
    const availableToolNames = [
      ...this.getTools()
        // child MCP 必须与 parent 启动 snapshot 使用同一批 descriptor，
        // 不能从另一份 registry 视图重新推导。
        .filter((tool) => tool.permission?.permission !== "mcp")
        .map((tool) => tool.name),
      ...parentAllowedMcpToolNames,
    ];
    // 非 Explore child 保留 Agent（只能派发前台 Explore）；Explore 与孙 agent 不再派发。
    const mayDispatch =
      !isBuiltInExploreAgentProfile(request.profile) && this.config.taskType !== "subagent_child";
    return appendCoordinatorResponseTool(
      [...new Set(availableToolNames)]
        .filter((toolName) => mayDispatch || !isSubagentDispatchToolName(toolName))
        .filter((toolName) => filterSubagentChildToolNames([toolName], disallowedRules).length > 0),
      disallowedRules,
    );
  }
  if (request.allowedTools.length > 0) {
    const parentHasCodeSearch =
      this.registry.has(FIND_FILES_TOOL_NAME) && this.registry.has(SEARCH_CODE_TOOL_NAME);
    const explicitTools = parentHasCodeSearch
      ? withCodeSearchEquivalents(request.allowedTools)
      : request.allowedTools;
    return appendCoordinatorResponseTool(
      filterSubagentChildToolNames(explicitTools, disallowedRules),
      disallowedRules,
    );
  }
  return appendCoordinatorResponseTool([], disallowedRules);
}

function filterMcpToolNamesByParentAllowlist(
  toolNames: readonly string[],
  parentAllowlist: readonly string[] | undefined,
): readonly string[] {
  if (parentAllowlist === undefined) return toolNames;
  const allowed = new Set(parentAllowlist);
  return toolNames.filter((toolName) => allowed.has(toolName));
}

function isModelVisibleMcpToolName(toolName: string): boolean {
  return toolName.startsWith("mcp__");
}

function appendCoordinatorResponseTool(
  toolNames: readonly string[],
  disallowedRules: Parameters<typeof filterSubagentChildToolNames>[1],
): readonly string[] {
  const result = [...toolNames];
  // child 控制通道不受 profile 工具列表约束；全局 toolDisallowlist 仍在 runtime 注册边界生效。
  if (!result.includes(RESPOND_TO_COORDINATOR_TOOL_NAME)) {
    result.push(RESPOND_TO_COORDINATOR_TOOL_NAME);
  }
  // 父 agent 可以通过 SendMessage 要求 child 压缩上下文，所以 Compact 同样不受 profile
  // 工具列表约束；但 profile 显式禁用时尊重禁用。
  if (
    !result.includes(COMPACT_TOOL_NAME) &&
    filterSubagentChildToolNames([COMPACT_TOOL_NAME], disallowedRules).length > 0
  ) {
    result.push(COMPACT_TOOL_NAME);
  }
  return result;
}

interface ResolvedSubagentMcpAccess {
  config: { enabled?: boolean } | undefined;
  port: McpPort | undefined;
  parentSnapshot: McpConnectionSnapshot | undefined;
  snapshot: McpConnectionSnapshot | undefined;
}

async function resolveSubagentMcpAccess(
  this: AgentRuntimeInternal,
  request: ExploreSubagentRuntimeRequest,
  officialCuaServerNames: ReadonlySet<string>,
): Promise<ResolvedSubagentMcpAccess> {
  if (!shouldBorrowParentMcp(request)) {
    return { config: undefined, parentSnapshot: undefined, port: undefined, snapshot: undefined };
  }

  const scopedServerNames = request.profile.mcpServers?.length
    ? request.profile.mcpServers
    : undefined;
  if (!this.mcpPort || this.config.mcp?.enabled === false) {
    if (scopedServerNames) {
      throw createSubagentMcpUnavailableError(request);
    }
    return {
      config: this.config.mcp?.enabled === false ? { enabled: false } : undefined,
      parentSnapshot: undefined,
      port: undefined,
      snapshot: undefined,
    };
  }

  // child 不拥有连接生命周期，只能复用 parent constructor 已创建的启动快照。
  const parentStartupSnapshot = await this.mcpStartupPromise;
  if (!parentStartupSnapshot) {
    if (scopedServerNames) {
      throw createSubagentMcpUnavailableError(request);
    }
    return { config: undefined, parentSnapshot: undefined, port: undefined, snapshot: undefined };
  }

  const unavailableScopedServerNames = (scopedServerNames ?? []).filter(
    (serverName) => parentStartupSnapshot.statuses[serverName]?.status !== "connected",
  );
  if (unavailableScopedServerNames.length > 0) {
    throw createCoreError(
      CoreErrorType.ConfigurationError,
      `Required MCP server is not connected: ${unavailableScopedServerNames.join(", ")}`,
      {
        context: {
          agentType: request.agentType,
          missingMcpServers: unavailableScopedServerNames,
        },
        recoverable: true,
      },
    );
  }

  const borrowed = createBorrowedSubagentMcpAccess(
    this.mcpPort,
    parentStartupSnapshot,
    scopedServerNames,
    officialCuaServerNames,
  );
  return {
    config: { enabled: true },
    parentSnapshot: parentStartupSnapshot,
    port: borrowed.port,
    snapshot: borrowed.snapshot,
  };
}

function shouldBorrowParentMcp(request: ExploreSubagentRuntimeRequest): boolean {
  if ((request.profile.mcpServers?.length ?? 0) > 0) return true;
  if (request.allowedTools.length === 0 || request.allowedTools.includes("*")) return true;
  return request.allowedTools.some((toolName) => {
    const normalized = toolName.trim();
    return isConcreteMcpToolName(normalized) || isMcpServerSelector(normalized);
  });
}

function validateSubagentMcpRequirements(
  request: ExploreSubagentRuntimeRequest,
  effectiveAllowedTools: readonly string[],
  access: ResolvedSubagentMcpAccess,
): void {
  const inheritsAvailableTools =
    request.allowedTools.length === 0 || request.allowedTools.includes("*");
  if (inheritsAvailableTools) return;

  const normalizedAllowedTools = effectiveAllowedTools.map((toolName) => toolName.trim());
  const requiredToolNames = normalizedAllowedTools.filter(isConcreteMcpToolName);
  const requiredServerNames = extractRequiredMcpServerNames(
    normalizedAllowedTools.filter(isMcpServerSelector),
  );
  if (requiredToolNames.length === 0 && requiredServerNames.length === 0) return;
  if (!access.port || !access.snapshot) {
    throw createSubagentMcpUnavailableError(request, requiredToolNames);
  }
  const snapshot = access.snapshot;

  const unavailableServerNames = requiredServerNames.filter(
    (serverName) => !matchesRequiredMcpServer(serverName, snapshot.statuses),
  );
  if (unavailableServerNames.length > 0) {
    throw createCoreError(
      CoreErrorType.ConfigurationError,
      `Required MCP server is not connected: ${unavailableServerNames.join(", ")}`,
      {
        context: {
          agentType: request.agentType,
          missingMcpServers: unavailableServerNames,
        },
        recoverable: true,
      },
    );
  }

  const visibleToolNames = new Set(snapshot.tools.map((descriptor) => toMcpToolName(descriptor)));
  const missingToolNames = requiredToolNames.filter((toolName) => !visibleToolNames.has(toolName));
  if (missingToolNames.length === 0) return;

  throw createCoreError(
    CoreErrorType.ConfigurationError,
    `Required MCP tool is not available in the parent startup snapshot: ${missingToolNames.join(", ")}`,
    {
      context: {
        agentType: request.agentType,
        missingMcpTools: missingToolNames,
      },
      recoverable: true,
    },
  );
}

function isConcreteMcpToolName(toolName: string): boolean {
  return isModelVisibleMcpToolName(toolName) && !isMcpServerSelector(toolName);
}

function isMcpServerSelector(toolName: string): boolean {
  return toolName === "mcp" || (toolName.startsWith("mcp__") && toolName.endsWith("__*"));
}

function createSubagentMcpUnavailableError(
  request: ExploreSubagentRuntimeRequest,
  requiredToolNames: readonly string[] = [],
) {
  return createCoreError(
    CoreErrorType.ConfigurationError,
    "Subagent MCP is unavailable because the parent startup snapshot is unavailable",
    {
      context: {
        agentType: request.agentType,
        requiredMcpTools: requiredToolNames,
      },
      recoverable: true,
    },
  );
}

function resolveSubagentSkillPort(
  parentSkillPort: SkillPort | undefined,
  skillNames: readonly string[] | undefined,
  cuaPolicy: OfficialCuaPolicy,
): SkillPort | undefined {
  if (!parentSkillPort) {
    return parentSkillPort;
  }
  return new FilteredSkillPort(
    parentSkillPort,
    skillNames && skillNames.length > 0 ? new Set(skillNames) : undefined,
    cuaPolicy,
  );
}

class FilteredSkillPort implements SkillPort {
  constructor(
    private readonly parent: SkillPort,
    private readonly allowedSkills: ReadonlySet<string> | undefined,
    private readonly cuaPolicy: OfficialCuaPolicy,
  ) {}

  async discoverSkills(
    request: Parameters<SkillPort["discoverSkills"]>[0],
    options?: SkillOperationOptions,
  ): Promise<SkillLoadOutcome> {
    const outcome = await this.parent.discoverSkills(request, options);
    const skills = outcome.skills.filter((skill) => this.isAllowedSkill(skill));
    return {
      ...outcome,
      skills,
      totalDiscovered: skills.length,
    };
  }

  async loadSkill(
    request: Parameters<SkillPort["loadSkill"]>[0],
    options?: SkillOperationOptions,
  ): Promise<SkillContent> {
    if (
      this.cuaPolicy.isOfficialSkillRequest(request.name) ||
      (await this.hasUniqueOfficialSkillMatch(request, options))
    ) {
      throw createSubagentComputerUseUnavailableError(request.name);
    }
    const resolvedName = await this.resolveAllowedSkillRequestName(request, options);
    if (!resolvedName) {
      throw createCoreError(
        CoreErrorType.ToolExecutionFailed,
        "Skill is not allowed for subagent",
        {
          context: {
            allowedSkills: this.allowedSkills ? [...this.allowedSkills] : [],
            skill: request.name,
            toolName: "Skill",
          },
          recoverable: true,
        },
      );
    }
    return this.parent.loadSkill({ ...request, name: resolvedName }, options);
  }

  private async hasUniqueOfficialSkillMatch(
    request: Parameters<SkillPort["loadSkill"]>[0],
    options?: SkillOperationOptions,
  ): Promise<boolean> {
    if (request.name.includes(":")) return false;
    const outcome = await this.parent.discoverSkills(
      {
        workingDirectory: request.workingDirectory,
        roots: request.roots,
        trace: request.trace,
      },
      options,
    );
    return isUniqueOfficialSkillRequest(outcome.skills, request.name, this.cuaPolicy);
  }

  private isAllowedSkill(skill: SkillContent["metadata"]): boolean {
    if (this.cuaPolicy.isOfficialSkill(skill)) return false;
    return (
      this.allowedSkills === undefined ||
      this.allowedSkills.has(skill.name) ||
      (skill.qualifiedName !== undefined && this.allowedSkills.has(skill.qualifiedName))
    );
  }

  private async resolveAllowedSkillRequestName(
    request: Parameters<SkillPort["loadSkill"]>[0],
    options?: SkillOperationOptions,
  ): Promise<string | undefined> {
    const outcome = await this.discoverSkills(
      {
        workingDirectory: request.workingDirectory,
        roots: request.roots,
        trace: request.trace,
      },
      options,
    );
    const matches = outcome.skills.filter((skill) => matchesSkillRequestName(skill, request.name));
    if (matches.length === 0) {
      return undefined;
    }
    if (matches.length > 1) {
      throw createCoreError(
        CoreErrorType.ToolExecutionFailed,
        "Skill name is ambiguous for subagent; use the fully qualified skill name",
        {
          context: {
            allowedSkills: this.allowedSkills ? [...this.allowedSkills] : [],
            matchingSkills: matches.map((skill) => skill.qualifiedName ?? skill.name),
            skill: request.name,
            toolName: "Skill",
          },
          recoverable: true,
        },
      );
    }
    // 可见 skills 列表会把 plugin skill 展示为 qualified name，并声明 bare alias 也可加载。
    // 子 agent 按 bare alias 调用时，先绑定回过滤后的 metadata，避免父端按全局同名 skill 误加载。
    return matches[0]?.qualifiedName ?? matches[0]?.name;
  }
}

async function validateSubagentComputerUseConfiguration(
  request: ExploreSubagentRuntimeRequest,
  cuaPolicy: OfficialCuaPolicy,
  parentSkillPort: SkillPort | undefined,
): Promise<void> {
  const explicitServer = request.profile.mcpServers?.find((serverName) =>
    cuaPolicy.serverNames.has(serverName.trim()),
  );
  const explicitTool = request.allowedTools.find(
    (toolName) =>
      cuaPolicy.isOfficialToolRequest(toolName) || cuaPolicy.isOfficialServerSelector(toolName),
  );
  let explicitSkill = request.profile.skills?.find((skillName) =>
    cuaPolicy.isOfficialSkillRequest(skillName),
  );
  if (!explicitSkill && parentSkillPort && request.profile.skills?.length) {
    try {
      const outcome = await parentSkillPort.discoverSkills({
        workingDirectory: request.workingDirectory,
        trace: request.traceContext,
      });
      explicitSkill = request.profile.skills.find((skillName) =>
        isUniqueOfficialSkillRequest(outcome.skills, skillName, cuaPolicy),
      );
    } catch {
      // Skill discovery remains lazy; a later Skill load will surface its own error.
    }
  }
  if (!explicitServer && !explicitTool && !explicitSkill) return;

  throw createCoreError(
    CoreErrorType.ConfigurationError,
    SUBAGENT_COMPUTER_USE_UNAVAILABLE_MESSAGE,
    {
      context: {
        agentType: request.agentType,
        code: SUBAGENT_COMPUTER_USE_UNAVAILABLE_CODE,
        ...(explicitServer ? { mcpServer: explicitServer } : {}),
        ...(explicitTool ? { mcpTool: explicitTool } : {}),
        ...(explicitSkill ? { skill: explicitSkill } : {}),
      },
      recoverable: true,
    },
  );
}

export function isUniqueOfficialSkillRequest(
  skills: readonly SkillContent["metadata"][],
  requestName: string,
  cuaPolicy: Pick<OfficialCuaPolicy, "isOfficialSkill">,
): boolean {
  if (requestName.includes(":")) return false;
  const matches = skills.filter((skill) => matchesSkillRequestName(skill, requestName));
  return matches.length === 1 && matches[0] !== undefined && cuaPolicy.isOfficialSkill(matches[0]);
}

function createSubagentComputerUseUnavailableError(skillName: string) {
  return createCoreError(
    CoreErrorType.ToolExecutionFailed,
    SUBAGENT_COMPUTER_USE_UNAVAILABLE_MESSAGE,
    {
      context: {
        code: SUBAGENT_COMPUTER_USE_UNAVAILABLE_CODE,
        skill: skillName,
        toolName: "Skill",
      },
      recoverable: true,
    },
  );
}

function matchesSkillRequestName(skill: SkillContent["metadata"], requestName: string): boolean {
  return skill.name === requestName || skill.qualifiedName === requestName;
}

function traceStringAttribute(
  traceContext: { attributes?: Record<string, string | number | boolean> },
  key: string,
): string | undefined {
  const value = traceContext.attributes?.[key];
  return typeof value === "string" ? value : undefined;
}
