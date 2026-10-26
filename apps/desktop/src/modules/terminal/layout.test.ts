import { describe, expect, it } from "vitest";

import {
  closeNode,
  createLeaf,
  createTab,
  findLeaf,
  listLeaves,
  nextPaneId,
  paneCount,
  setSplitRatio,
  splitNode,
} from "./layout";

describe("terminal layout tree", () => {
  it("splits a leaf horizontally and vertically without losing panes", () => {
    const tab = createTab(1);
    const first = tab.root;
    const right = createLeaf("Terminal 1 · 2");
    const withRight = splitNode(first, tab.activePaneId, right, "horizontal");
    expect(paneCount(withRight)).toBe(2);
    expect(listLeaves(withRight).map((leaf) => leaf.id)).toEqual([
      tab.activePaneId,
      right.id,
    ]);

    const down = createLeaf("Terminal 1 · 3");
    const withDown = splitNode(withRight, right.id, down, "vertical");
    expect(paneCount(withDown)).toBe(3);
    expect(findLeaf(withDown, down.id)?.title).toBe("Terminal 1 · 3");
    expect(withDown.kind).toBe("split");
    if (withDown.kind === "split") {
      expect(withDown.direction).toBe("horizontal");
      expect(withDown.second.kind).toBe("split");
      if (withDown.second.kind === "split") {
        expect(withDown.second.direction).toBe("vertical");
      }
    }
  });

  it("collapses a split when one child closes", () => {
    const tab = createTab(1);
    const right = createLeaf("Terminal 1 · 2");
    const root = splitNode(tab.root, tab.activePaneId, right, "horizontal");
    const afterClose = closeNode(root, right.id);
    expect(afterClose).not.toBeNull();
    expect(paneCount(afterClose!)).toBe(1);
    expect(findLeaf(afterClose!, right.id)).toBeUndefined();
  });

  it("returns null when the final pane closes and ignores unknown panes", () => {
    const tab = createTab(1);
    expect(closeNode(tab.root, "missing-pane")).toBe(tab.root);
    expect(closeNode(tab.root, tab.activePaneId)).toBeNull();
  });

  it("navigates panes in tree order with wraparound", () => {
    const tab = createTab(1);
    const right = createLeaf("Terminal 1 · 2");
    const root = splitNode(tab.root, tab.activePaneId, right, "horizontal");
    const down = createLeaf("Terminal 1 · 3");
    const finalRoot = splitNode(root, right.id, down, "vertical");
    expect(nextPaneId(finalRoot, tab.activePaneId, 1)).toBe(right.id);
    expect(nextPaneId(finalRoot, right.id, 1)).toBe(down.id);
    expect(nextPaneId(finalRoot, down.id, 1)).toBe(tab.activePaneId);
    expect(nextPaneId(finalRoot, tab.activePaneId, -1)).toBe(down.id);
  });

  it("clamps split ratios without changing the tree", () => {
    const tab = createTab(1);
    const right = createLeaf("Terminal 1 · 2");
    const root = splitNode(tab.root, tab.activePaneId, right, "horizontal");
    expect(root.kind).toBe("split");
    if (root.kind !== "split") return;
    const narrower = setSplitRatio(root, root.id, 0.1);
    expect(narrower.kind).toBe("split");
    if (narrower.kind === "split") expect(narrower.ratio).toBe(0.15);
    const wider = setSplitRatio(root, root.id, 0.95);
    if (wider.kind === "split") expect(wider.ratio).toBe(0.85);
    expect(setSplitRatio(root, "missing-split", 0.4)).toBe(root);
  });

  it("keeps four independent panes and stable ids", () => {
    const tab = createTab(1);
    let root = tab.root;
    const leaves = [createLeaf("two"), createLeaf("three"), createLeaf("four")];
    root = splitNode(root, tab.activePaneId, leaves[0]!, "horizontal");
    root = splitNode(root, tab.activePaneId, leaves[1]!, "vertical");
    root = splitNode(root, leaves[0]!.id, leaves[2]!, "horizontal");
    const all = listLeaves(root);
    expect(all).toHaveLength(4);
    expect(new Set(all.map((leaf) => leaf.id)).size).toBe(4);
  });
});
