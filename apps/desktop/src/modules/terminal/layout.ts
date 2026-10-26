/** Pure pane-tree and tab helpers for the terminal workspace. */

export type SplitDirection = "horizontal" | "vertical";

export type TerminalLeaf = {
  kind: "leaf";
  id: string;
  title: string;
  sessionId: string | null;
  historyId: string;
  cwd: string | null;
  shell: string | null;
  scrollOffset: number;
  sessionLost: boolean;
};

export type TerminalSplit = {
  kind: "split";
  id: string;
  direction: SplitDirection;
  ratio: number;
  first: LayoutNode;
  second: LayoutNode;
};

export type LayoutNode = TerminalLeaf | TerminalSplit;

export type TerminalTab = {
  id: string;
  title: string;
  root: LayoutNode;
  activePaneId: string;
  zoomedPaneId: string | null;
};

let nextId = 0;

export function createId(prefix: string): string {
  nextId += 1;
  return `${prefix}-${nextId}`;
}

export function createLeaf(title: string): TerminalLeaf {
  const id = createId("pane");
  return {
    kind: "leaf",
    id,
    title,
    sessionId: null,
    historyId: id,
    cwd: null,
    shell: null,
    scrollOffset: 0,
    sessionLost: false,
  };
}

export function createTab(number: number): TerminalTab {
  const leaf = createLeaf(`Terminal ${number}`);
  return {
    id: createId("tab"),
    title: `Terminal ${number}`,
    root: leaf,
    activePaneId: leaf.id,
    zoomedPaneId: null,
  };
}

export function listLeaves(node: LayoutNode): TerminalLeaf[] {
  if (node.kind === "leaf") return [node];
  return [...listLeaves(node.first), ...listLeaves(node.second)];
}

export function findLeaf(
  node: LayoutNode,
  paneId: string,
): TerminalLeaf | undefined {
  if (node.kind === "leaf") return node.id === paneId ? node : undefined;
  return findLeaf(node.first, paneId) ?? findLeaf(node.second, paneId);
}

export function splitNode(
  root: LayoutNode,
  targetPaneId: string,
  newLeaf: TerminalLeaf,
  direction: SplitDirection,
): LayoutNode {
  if (!findLeaf(root, targetPaneId)) return root;
  return splitAt(root, targetPaneId, newLeaf, direction);
}

function splitAt(
  node: LayoutNode,
  targetPaneId: string,
  newLeaf: TerminalLeaf,
  direction: SplitDirection,
): LayoutNode {
  if (node.kind === "leaf") {
    if (node.id !== targetPaneId) return node;
    return {
      kind: "split",
      id: createId("split"),
      direction,
      ratio: 0.5,
      first: node,
      second: newLeaf,
    };
  }

  const first = splitAt(node.first, targetPaneId, newLeaf, direction);
  if (first !== node.first) {
    return { ...node, first };
  }
  const second = splitAt(node.second, targetPaneId, newLeaf, direction);
  if (second !== node.second) {
    return { ...node, second };
  }
  return node;
}

export function closeNode(
  root: LayoutNode,
  targetPaneId: string,
): LayoutNode | null {
  if (root.kind === "leaf") {
    return root.id === targetPaneId ? null : root;
  }

  const first = closeNode(root.first, targetPaneId);
  const second = closeNode(root.second, targetPaneId);
  if (first === root.first && second === root.second) return root;
  if (first === null && second === null) return null;
  if (first === null) return second;
  if (second === null) return first;
  return { ...root, first, second };
}

export function setSplitRatio(
  root: LayoutNode,
  splitId: string,
  ratio: number,
): LayoutNode {
  const clamped = Math.min(0.85, Math.max(0.15, ratio));
  if (root.kind === "leaf") return root;
  if (root.id === splitId) return { ...root, ratio: clamped };
  const first = setSplitRatio(root.first, splitId, clamped);
  if (first !== root.first) return { ...root, first };
  const second = setSplitRatio(root.second, splitId, clamped);
  if (second !== root.second) return { ...root, second };
  return root;
}

export function nextPaneId(
  root: LayoutNode,
  currentPaneId: string,
  delta: 1 | -1,
): string {
  const leaves = listLeaves(root);
  if (leaves.length === 0) return currentPaneId;
  const index = leaves.findIndex((leaf) => leaf.id === currentPaneId);
  if (index < 0) return leaves[0]?.id ?? currentPaneId;
  const next = (index + delta + leaves.length) % leaves.length;
  return leaves[next]?.id ?? currentPaneId;
}

export function paneCount(node: LayoutNode): number {
  return listLeaves(node).length;
}

export function updateLeaf(
  node: LayoutNode,
  paneId: string,
  update: (leaf: TerminalLeaf) => TerminalLeaf,
): LayoutNode {
  if (node.kind === "leaf") {
    return node.id === paneId ? update(node) : node;
  }
  const first = updateLeaf(node.first, paneId, update);
  if (first !== node.first) return { ...node, first };
  const second = updateLeaf(node.second, paneId, update);
  if (second !== node.second) return { ...node, second };
  return node;
}
