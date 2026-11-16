# codemode：脚本内调用工具

## 背景

模型每走一步都要重发整段上下文；多步 MCP 流程（Blender、Fuse、browser DOM）还会把大块中间结果
原样塞进上下文。`codemode` 让模型写一段 JavaScript，脚本里用 `tools.<name>(args)` 调用其他工具，
只有脚本输出回到模型。沙箱来自 pi 的 `@earendil-works/pi-codemode`（MIT，QuickJS/WASM，
每次执行一个 worker 线程），本仓库只负责与工具管线、持久化和 UI 的对接。

现有 `node_repl`（`js`）是完整 Node 权限、不经工具管线，不能替代本工具。

## 目标

- 嵌套调用与模型直接调用走**同一条** `executeToolCall` 管线：deferred 解析、输入校验、
  PreToolUse hook、权限判断与审批弹窗、超时与取消、secrets 脱敏、结果预算、PostToolUse hook。
  codemode 不能成为绕过权限的通道。
- 设置 `codemodeMode: off | on | only`，缺省 `on`。主 Agent 与子 Agent 都可用；子 Agent 只能调用
  自身 registry 中的工具。
- `store(key, value)` / `load(key)` 跨调用保存小 JSON 状态，随分支/rewind/resume 走。
- GPUI：脚本以 JS 高亮显示；完成后展开行列出嵌套调用。

## 非目标

- pi 的 `models` API（分类器、图像生成）。
- 嵌套调用的实时行或进度列表（v1 只在完成后列出）。
- 脚本内再调用 codemode、启动子 Agent 或结束当前轮次。

## 设计

### 所有者与事件顺序

```text
model ──codemode{code}──► turn loop（ToolCallScheduled → row + ToolPart，与普通工具一致）
  call-runner(codemode) ─► handler ─► CodemodeSandbox.execute(code, {tools, store})
      QuickJS worker: tools.Read({...}) ─► host bridge（handler 内）
        ─► context.nestedTools.execute(name, input, signal)
             ─► executeToolCall(...) 完整管线，id = `${parentId}~cm${n}`
                  事件带 parentToolCallId + source:"codemode"（无 ToolCallScheduled）
             ◄─ 脱敏、预算后的模型可见内容
  handler ◄─ {output, storeWrites, calls}
  结果 ─► ToolPart（metadata.codemodeStore）+ output.display {kind:"codemode", calls}
```

- **嵌套调用不投影为行**：V4 projection 只从 `ToolCallScheduled` 建工具行；`source === "codemode"`
  的 Started/Progress/Result/Error 与镜像的子 Agent 事件一样被跳过。
- **嵌套调用不进模型历史**：ToolPart 只由 turn loop 落盘，嵌套调用的结果只经脚本输出回到模型。
- **权限弹窗**：嵌套调用的 `PermissionRequested` 进入同一会话的 `pendingInteractions`，GPUI 不依赖行即可展示。
- **读状态**：嵌套 Read 更新运行时的 read-file state（之后的 Edit/Write 可用），但不写进 ToolPart
  元数据，因此 resume 后不恢复；需要时模型重新 Read。

### NestedToolPort

`ToolExecutionContext.nestedTools`，由 call-runner 用自身 deps 构造，只在当前调用期间有效：

- `listTools()`：registry 的全部模型契约（含 deferred MCP），去掉禁止项。
- `execute(name, input, signal)`：调用 `executeToolCall`，沿用父调用的 model、toolSearch、
  权限上下文；trace 为子上下文；取消信号与父调用联动。

**禁止嵌套调用**：`codemode` 自身、`Agent`、`SendMessage`、`AskUserQuestion`、`EnterPlanMode`、
`ExitPlanMode`、`Compact`、`Speak`、`ToolSearch`/`LocalToolSearch`、`DeferredToolCall`、`Task`、
`RespondToCoordinator`、`submit_result`、`escalate`，以及 `stopTurnOnSuccess` 的工具
和 `providerVisible: false` 的工具（Speak 没有 `stopTurnOnSuccess`，所以显式列出）。脚本内用 `searchTools`/`describeTool` 发现工具。pi 的 `tools`
代理对未知成员（包括 `typeof tools.X`）直接抛错，脚本应先用 `searchTools` 或 `ALL_TOOLS` 确认。

**并发**：只读或 `concurrentSafe` 的嵌套调用可并行（上限沿用 executor `maxConcurrency`）；
其他调用在每个脚本内串行，保持 scheduler 的副作用安全约定。排队中的调用随轮次取消立即放弃，不再启动。

**返回值**：模型可见内容（已脱敏、已按预算截断）。有非空 `structuredContent` 时返回该对象；
只有文本时返回拼接后的字符串；含图片时返回 `[{type:"text", text} | {type:"image", data, mimeType}]`
数组。失败、拒绝、无效调用在脚本里 reject 为 `Error`（message 已脱敏）。

### 工具本体

- 输入 `{ code: string }`，首行可选 `// @options: {"max_output_tokens": N, "timeout_ms": N}`，
  由 `parseCodemodeSource` 解析。
- 每次执行新建一个 `CodemodeSandbox`（工具绑定当前调用）；每进程同时最多 2 个脚本运行（环境变量
  `GCODE_CODEMODE_MAX_WORKERS` 可调），排队最多 60 秒；VM 内存上限 256 MB。
- `on` 模式下模型本来就有其他工具的声明，codemode 说明不重复列出；`only` 模式见下。
  脚本随时可用 `searchTools(query)`、`describeTool(name)` 获取声明（含 deferred MCP 工具）。
- 输出：`Script completed|failed in Xs`，之后是文本项（多项时带 `==> text N/M <==`）、
  `<console_output>`、失败时 `Script error:`。超过 `max_output_tokens`（缺省 10000，按 4 字符/token
  估算）时保留首尾并注明省略的字符数（不写临时文件）；预算为 0 时只返回省略说明。最多 4 张图片作为图片内容返回。
- 嵌套调用在 hook 中附加的上下文追加在输出之后。
- `output.display = {kind:"codemode", ok, calls[{name, status, durationMs}], truncated?}`，最多 100 条，
  供 UI 列出嵌套调用；`shared` 的 `toolResultDisplaySchema` 与 contracts 的成员同步。
- 元数据：`needsApproval: false`（每个嵌套调用自己判权限），非只读。plan mode 下允许调用本工具，
  嵌套写操作由各自的权限判断拒绝。
- 超时：工具本身无缺省超时（executor `timeout.kind = "none"`），因为嵌套调用可能在等用户审批；
  脚本可用 `timeout_ms` 自设上限。取消随轮次中止。

### store

- 成功执行的 `storeWrites`（`{set, delete}`）写进本次 ToolPart `metadata.codemodeStore`。
- 运行时按 `activeSessionMessages` 顺序重放这些增量得到当前 store；resume、rewind 时重建，
  与 read-file state 相同的模式。失败的脚本不写。
- 单值 256 Ki 字符、总量 1 Mi 字符（沙箱强制）。

### 模式

- `off`：不注册 codemode。
- `on`：codemode 与其他工具一起声明给模型。
- `only`：发给模型的工具只有 codemode 与轮次控制类工具（`Agent`、`Task`、`SendMessage`、
  `ListAgents`、`AskUserQuestion`、计划模式工具、`TodoWrite`/`TodoRead`、`Speak`、`Compact`、
  `Skill`、`RespondToCoordinator`、`submit_result`、`escalate`）；`ToolSearch` 等搜索工具也去掉。
  被隐藏工具的 `renderDeclarations` 声明内联进 codemode 说明，预算 12000 字符（约 3000 token），
  超出预算的只列名字。其他工具只能经脚本调用。
- `only` 模式下请求按 `toolSearch: off` 构建：不发 `deferLoading`、不开 provider 工具搜索，
  延迟的 MCP 工具同样进入 codemode 说明；`toolSearchMode` 只对 `off`/`on` 生效。
- 设置链路与 `toolSearchMode` 类似：GPUI 专属偏好 `gpuiCodemode`（存 `gpui_settings.json`，
  缺省 `on`，Settings › MCP Servers「Code mode」）→ `session/requestRuntimePreferences` 的
  `codemodeMode` → bootstrap 写入 runtime config 与会话记录；子 Agent 继承父会话。
  Host 不发该字段（Electron、旧 Host、CLI/TUI）时 core 按 `off` 处理，不改变既有工具列表。

### 打包

编译后的 backend exe 中：`packages/cli/src/codemode-worker.ts`（与 `main.ts` 同目录）作为额外的
Bun build entrypoint 嵌入，`workerUrl` 为 `"./codemode-worker.js"`；`quickjs.wasm` 与 ripgrep 一样拷到 exe 同级 `node_modules/quickjs-wasi/`，编译模式下
（`Bun.main` 位于 `B:/~BUN` 或 `/$bunfs`）按 `dirname(process.execPath)` 加载。源码运行用包的缺省值。

## 验收

- 嵌套 Read 成功且结果已脱敏；plan mode 下嵌套 Bash/Write 被拒，脚本收到拒绝。
- PreToolUse hook 的拒绝与改写输入生效。
- 禁止项抛错；轮次取消会取消进行中的嵌套调用；非只读调用串行。
- 嵌套调用不产生 V4 行、不产生 ToolPart。
- store：写入后可读、resume 后仍在、rewind 到更早消息后丢弃之后的写入。
- `only` 模式的工具暴露；Explore 子 Agent 有 codemode 且只能调用自身工具。
- 编译 exe 中脚本可运行（worker 与 wasm 可加载）：`apps/zcode-gpui/scripts/backend_codemode_smoke.ts`
  以 mock 模型覆盖 off/on/only 三种模式与嵌套 Read，源码与 `ZCODE_GPUI_SMOKE_BACKEND` 编译产物都要过。
- 单测：`core/tests/codemode-tool.test.ts`；GPUI：`codemode_card`、`codemode_setting` 与 reverse RPC 测试。

## 来源

沙箱与脚本 API 来自 pi（https://github.com/earendil-works/pi，`packages/codemode`，MIT）；
工具说明文案改写自其 `packages/coding-agent/docs/codemode.md`。
