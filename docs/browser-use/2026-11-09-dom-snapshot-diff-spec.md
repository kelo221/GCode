# domSnapshot diff 模式 Spec（2026-11-09）

## 背景

模型读取页面的主路径是 `tab.playwright.domSnapshot()`：返回归一化后的 Playwright AI/ARIA
快照文本（2 空格缩进的 `- role "name"` 行，ref/cursor 已删除，匿名容器已压平，见
`packages/browser-commands/src/browserPlaywrightDomSnapshot.ts`）。同一页面上做一次动作后再取
全量快照，大部分行与上一次相同，浪费 token。参考 anomalyco/browser-control 的
`snapshot({ diff })`，增加只返回变化部分的选项。

## 产品规则

- API：`tab.playwright.domSnapshot(options?: { diff?: boolean }): Promise<string>`。
  不传或 `diff: false` 时行为与此前完全一致（返回全量文本）。
- 基线：每次 `domSnapshot()`（无论是否 diff）成功后，都把本次全量文本记为该 tab 的基线。
- `diff: true` 时：
  - 无基线 → 返回全量文本，首行头 `[domSnapshot diff: no previous snapshot of this tab; full snapshot follows]`。
  - 与基线完全相同 → 返回 `[domSnapshot diff: no changes since previous snapshot of this tab]`。
  - 新增行超过新快照行数 50%，或删除行超过旧快照行数 50%（通常是导航或大面积重渲染），
    或 diff 计算超时 → 返回全量文本，首行头说明原因（`page changed substantially`）。
  - 否则返回差异：首行头 `[domSnapshot diff vs previous snapshot of this tab: +A -D, N unchanged lines omitted]`，
    之后每个变化行带标记（新增 `+ `，删除 `- `），并在其前面输出一次未变化的祖先行（前缀两个空格）
    作为树上下文。新增行的祖先取自新快照，删除行的祖先取自旧快照。
- ref 每次快照都会重新编号且已从文本中删除，因此 diff 只比较文本行，不依赖 ref。

## 状态所有者

- 基线缓存只存在于 agent 侧 browser-client（`apps/zcode-cli/packages/core/src/browser-client/dom-snapshot-diff.ts`），
  生命周期等于 REPL 进程。协议、zod schema、contracts 镜像与各 backend（IAB / CLI CDP / 扩展）均不变。
- `Tab` 对象每次查询都会重建，因此缓存不能挂在 `PlaywrightAPI` 实例上，按 tab key
  （`tabId`，默认 tab 用 `"default"`）存放在有界 LRU（16 项）中。

## 验收场景

1. 首次 `domSnapshot({ diff: true })` 返回带 "no previous snapshot" 头的全量文本。
2. 打开对话框后 diff 只包含新增的对话框子树及其祖先上下文行。
3. 删除、修改（删旧行 + 增新行）均正确标记。
4. 导航到不同页面后 diff 回退为全量。
5. 两个 tab 的基线互不影响；全量调用会刷新基线；超过 16 个 tab 时最久未用的基线被淘汰。
