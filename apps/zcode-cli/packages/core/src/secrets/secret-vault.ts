/**
 * GCode 密钥库在 agent 进程内的唯一持有者（apps/zcode-gpui/SECRETS.md）。
 *
 * GPUI 每个 workspace 一个 agent 进程，并已按 workspace 过滤，所以这里是进程级整表：
 * `secrets/sync` 整表替换；Bash 执行端口每条命令现取 env，stdio MCP 拉起时现取，
 * 工具结果与进度事件在落盘、推流、进模型之前按值脱敏。只在内存，不落盘、不记日志。
 */

export interface SecretVaultEntry {
  name: string;
  value: string;
}

/** 短于此长度的值不脱敏：误伤普通输出的概率太高（UI 保存时提示）。 */
export const MIN_REDACTED_SECRET_LENGTH = 6;
export const SECRET_REDACTION_MASK = "***";

let env: Readonly<Record<string, string>> = {};
/** 长值在前，避免一个值是另一个值的子串时只替换掉一部分。 */
let redactionValues: readonly string[] = [];

export function replaceSecretVault(entries: readonly SecretVaultEntry[]): void {
  const next: Record<string, string> = {};
  for (const { name, value } of entries) next[name] = value;
  env = Object.freeze(next);
  redactionValues = [...new Set(entries.map((entry) => entry.value))]
    .filter((value) => value.length >= MIN_REDACTED_SECRET_LENGTH)
    .sort((a, b) => b.length - a.length);
}

export function secretVaultEnv(): Record<string, string> {
  return { ...env };
}

export function secretVaultSize(): number {
  return Object.keys(env).length;
}

export function redactSecretText(text: string): string {
  if (redactionValues.length === 0) return text;
  let result = text;
  for (const value of redactionValues) {
    if (result.includes(value)) result = result.split(value).join(SECRET_REDACTION_MASK);
  }
  return result;
}

/**
 * 结构化载荷（工具输出对象、进度事件）逐个字符串脱敏；没有命中时返回原对象，
 * 不为每个事件复制一遍。
 */
export function redactSecretsDeep<T>(value: T): T {
  if (redactionValues.length === 0) return value;
  return redactNode(value, new WeakSet()) as T;
}

function redactNode(value: unknown, seen: WeakSet<object>): unknown {
  if (typeof value === "string") return redactSecretText(value);
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return value;
  seen.add(value);
  if (Array.isArray(value)) {
    let changed = false;
    const next = value.map((item) => {
      const redacted = redactNode(item, seen);
      if (redacted !== item) changed = true;
      return redacted;
    });
    return changed ? next : value;
  }
  // 只处理普通对象；Buffer、Date 等原样保留。
  const proto = Object.getPrototypeOf(value);
  if (proto !== Object.prototype && proto !== null) return value;
  let changed = false;
  const next: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(value)) {
    const redacted = redactNode(item, seen);
    if (redacted !== item) changed = true;
    next[key] = redacted;
  }
  return changed ? next : value;
}
