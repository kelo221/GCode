import { beforeEach, describe, expect, it } from "vitest";
import type { BrowserCommand, BrowserCommandResult } from "@zcode/contracts";
import { Tab } from "../src/browser-client/index.js";
import {
  renderDomSnapshotDiff,
  resetDomSnapshotBaselines,
  swapDomSnapshotBaseline,
} from "../src/browser-client/dom-snapshot-diff.js";

const PAGE = [
  "- banner:",
  '  - link "Home"',
  '  - button "Menu"',
  "- main:",
  '  - heading "Orders" [level=1]',
  "  - list:",
  '    - link "Order 1"',
  '    - link "Order 2"',
  '    - link "Order 3"',
  '    - link "Order 4"',
  "- contentinfo:",
  "  - text: Footer",
].join("\n");

function tabServing(snapshots: string[], tabId = "tab-1"): Tab {
  const execute = async (_command: BrowserCommand): Promise<BrowserCommandResult> => ({
    ok: true,
    value: snapshots.shift() ?? "",
    elapsedMs: 0,
  });
  return new Tab(execute, tabId);
}

beforeEach(() => resetDomSnapshotBaselines());

describe("renderDomSnapshotDiff", () => {
  it("returns the full snapshot with a header when there is no baseline", () => {
    const out = renderDomSnapshotDiff(undefined, PAGE);
    expect(out.split("\n")[0]).toContain("no previous snapshot of this tab");
    expect(out).toContain(PAGE);
  });

  it("reports no changes for identical snapshots", () => {
    expect(renderDomSnapshotDiff(PAGE, PAGE)).toBe(
      "[domSnapshot diff: no changes since previous snapshot of this tab]",
    );
  });

  it("shows an added line with its unchanged ancestors as context", () => {
    const next = PAGE.replace(
      '  - button "Menu"',
      '  - button "Menu" [expanded]\n    - menuitem "Logout"',
    );
    const out = renderDomSnapshotDiff(PAGE, next);
    expect(out.split("\n")).toEqual([
      "[domSnapshot diff vs previous snapshot of this tab: +2 -1, 10 unchanged lines omitted]",
      "  - banner:",
      '-   - button "Menu"',
      '+   - button "Menu" [expanded]',
      '+     - menuitem "Logout"',
    ]);
  });

  it("shows a removed line with its ancestors from the old tree", () => {
    const next = PAGE.replace('    - link "Order 2"\n', "");
    const out = renderDomSnapshotDiff(PAGE, next);
    expect(out.split("\n")).toEqual([
      "[domSnapshot diff vs previous snapshot of this tab: +0 -1, 9 unchanged lines omitted]",
      "  - main:",
      "    - list:",
      '-     - link "Order 2"',
    ]);
  });

  it("falls back to the full snapshot when most of the page changed", () => {
    const next = [
      "- main:",
      '  - heading "Checkout"',
      '  - button "Pay"',
      '  - textbox "Card"',
    ].join("\n");
    const out = renderDomSnapshotDiff(PAGE, next);
    expect(out.split("\n")[0]).toContain("page changed substantially");
    expect(out).toContain(next);
  });
});

describe("PlaywrightAPI.domSnapshot diff option", () => {
  it("keeps the plain call unchanged and uses it as the baseline", async () => {
    const changed = PAGE.replace('"Order 4"', '"Order 4 (shipped)"');
    const tab = tabServing([PAGE, changed]);
    expect(await tab.playwright.domSnapshot()).toBe(PAGE);
    const out = await tab.playwright.domSnapshot({ diff: true });
    expect(out).toContain('+     - link "Order 4 (shipped)"');
    expect(out).toContain('-     - link "Order 4"');
  });

  it("keeps baselines per tab and across rebuilt Tab objects", async () => {
    await tabServing([PAGE], "tab-a").playwright.domSnapshot();
    const other = await tabServing([PAGE], "tab-b").playwright.domSnapshot({ diff: true });
    expect(other).toContain("no previous snapshot of this tab");
    const same = await tabServing([PAGE], "tab-a").playwright.domSnapshot({ diff: true });
    expect(same).toContain("no changes");
  });

  it("rejects a non-boolean diff option", async () => {
    const tab = tabServing([PAGE]);
    await expect(tab.playwright.domSnapshot({ diff: "yes" as unknown as boolean })).rejects.toThrow(
      "diff option must be a boolean",
    );
  });
});

describe("baseline cache", () => {
  it("evicts the least recently used tab beyond 16 entries", () => {
    for (let index = 0; index < 17; index += 1) swapDomSnapshotBaseline(`tab-${index}`, PAGE);
    expect(swapDomSnapshotBaseline("tab-0", PAGE)).toBeUndefined();
    expect(swapDomSnapshotBaseline("tab-16", PAGE)).toBe(PAGE);
  });
});
