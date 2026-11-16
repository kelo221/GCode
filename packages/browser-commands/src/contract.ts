/**
 * browser-commands 对宿主公开的契约：宿主提供 `ControlledView`（页面导航 + CDP 直通），
 * 命令层负责把 `BrowserCommand` 执行成 `BrowserCommandResult`。
 * Desktop 内置浏览器与浏览器扩展共用这一层，宿主差异只通过 `ControlledView` 与运行时注入表达。
 */
export type {
  BrowserPoint,
  ControlledView,
  ControlledViewCdp,
  ControlledViewKeyInputEvent,
  ControlledViewWebContents,
} from "./browserCommandTypes.js";
export type { BrowserCommandLogger, BrowserCommandRuntime } from "./browserCommandRuntime.js";
export type { IabLocatorExecution } from "./browserPlaywrightLocatorExecutor.js";

export { configureBrowserCommandRuntime } from "./browserCommandRuntime.js";
export { executeBrowserCommandOnView, isAllowedBrowserUrl } from "./browserCommandExecutor.js";
export { executeIabPlaywrightLocator } from "./browserPlaywrightLocatorExecutor.js";
export {
  PLAYWRIGHT_DEFAULT_TIMEOUT_MS,
  normalizePlaywrightTimeout,
} from "./browserPlaywrightTimeout.js";
