import { diffArrays } from "diff";

/**
 * domSnapshot diff 模式：按 tab 记住上一次全量快照文本，只把变化行和其祖先上下文返回给模型。
 * Tab 对象每次查询都会重建，所以基线放在模块级有界缓存里，而不是 PlaywrightAPI 实例上。
 * 规则见 docs/browser-use/2026-11-09-dom-snapshot-diff-spec.md。
 */

const MAX_BASELINES = 16;
const CHURN_FALLBACK_RATIO = 0.5;
const DIFF_TIMEOUT_MS = 1_000;

const baselines = new Map<string, string>();

/** 记录 key 对应 tab 的最新全量快照，返回此前的基线；超出上限时淘汰最久未用的一项。 */
export function swapDomSnapshotBaseline(key: string, snapshot: string): string | undefined {
  const previous = baselines.get(key);
  baselines.delete(key);
  baselines.set(key, snapshot);
  while (baselines.size > MAX_BASELINES) {
    const oldest = baselines.keys().next().value;
    if (oldest === undefined) break;
    baselines.delete(oldest);
  }
  return previous;
}

/** 仅供测试：清空所有基线。 */
export function resetDomSnapshotBaselines(): void {
  baselines.clear();
}

type EntryKind = "same" | "add" | "del";

interface Entry {
  kind: EntryKind;
  line: string;
  indent: number;
  printed: boolean;
}

function splitLines(snapshot: string): string[] {
  return snapshot === "" ? [] : snapshot.split("\n");
}

function indentOf(line: string): number {
  return line.match(/^ */)?.[0].length ?? 0;
}

function fullWithHeader(reason: string, snapshot: string): string {
  return `[domSnapshot diff: ${reason}; full snapshot follows]\n${snapshot}`;
}

function popToParent(stack: Entry[], indent: number): void {
  while (stack.length > 0 && (stack.at(-1)?.indent ?? -1) >= indent) stack.pop();
}

function emit(output: string[], ancestors: Entry[], entry: Entry): void {
  for (const ancestor of ancestors) {
    if (ancestor.printed) continue;
    const marker = ancestor.kind === "add" ? "+ " : ancestor.kind === "del" ? "- " : "  ";
    output.push(marker + ancestor.line);
    ancestor.printed = true;
  }
  output.push((entry.kind === "add" ? "+ " : "- ") + entry.line);
  entry.printed = true;
}

/** previous 为 undefined 表示该 tab 还没有基线。 */
export function renderDomSnapshotDiff(previous: string | undefined, next: string): string {
  if (previous === undefined) return fullWithHeader("no previous snapshot of this tab", next);
  if (previous === next)
    return "[domSnapshot diff: no changes since previous snapshot of this tab]";

  const oldLines = splitLines(previous);
  const newLines = splitLines(next);
  const changes = diffArrays(oldLines, newLines, { timeout: DIFF_TIMEOUT_MS });
  if (!changes) return fullWithHeader("page changed substantially", next);

  const entries: Entry[] = changes.flatMap((change) => {
    const kind: EntryKind = change.added ? "add" : change.removed ? "del" : "same";
    return change.value.map((line) => ({ kind, line, indent: indentOf(line), printed: false }));
  });
  const added = entries.filter((entry) => entry.kind === "add").length;
  const removed = entries.filter((entry) => entry.kind === "del").length;
  if (
    added > newLines.length * CHURN_FALLBACK_RATIO ||
    removed > oldLines.length * CHURN_FALLBACK_RATIO
  ) {
    return fullWithHeader("page changed substantially", next);
  }

  // 新增行的祖先来自新树（same + add），删除行的祖先来自旧树（same + del）。
  const newStack: Entry[] = [];
  const oldStack: Entry[] = [];
  const body: string[] = [];
  for (const entry of entries) {
    if (entry.kind !== "del") {
      popToParent(newStack, entry.indent);
      if (entry.kind === "add") emit(body, newStack, entry);
      newStack.push(entry);
    }
    if (entry.kind !== "add") {
      popToParent(oldStack, entry.indent);
      if (entry.kind === "del") emit(body, oldStack, entry);
      oldStack.push(entry);
    }
  }
  const omitted = entries.filter((entry) => entry.kind === "same" && !entry.printed).length;
  const header = `[domSnapshot diff vs previous snapshot of this tab: +${added} -${removed}, ${omitted} unchanged lines omitted]`;
  return [header, ...body].join("\n");
}
