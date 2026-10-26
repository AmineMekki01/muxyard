/** Conversions between the in-memory pane tree and the persisted layout DTO. */

import type {
  TerminalLayoutNode,
  TerminalTabLayout,
  WorkspaceLayout,
} from "@muxyard/contracts";

import type { LayoutNode, TerminalTab } from "./layout";

export type LoadedWorkspace = {
  tabs: TerminalTab[];
  activeTabId: string | null;
};

export function toWorkspaceLayout(
  tabs: readonly TerminalTab[],
  activeTabId: string | null,
): WorkspaceLayout {
  return {
    version: 1,
    tabs: tabs.map(toTabLayout),
    active_tab_id: activeTabId,
  };
}

function toTabLayout(tab: TerminalTab): TerminalTabLayout {
  return {
    id: tab.id,
    title: tab.title,
    root: toLayoutNode(tab.root),
    active_pane_id: tab.activePaneId,
    zoomed_pane_id: tab.zoomedPaneId,
  };
}

function toLayoutNode(node: LayoutNode): TerminalLayoutNode {
  if (node.kind === "leaf") {
    return {
      kind: "leaf",
      id: node.id,
      title: node.title,
      session_id: node.sessionId,
      history_id: node.historyId,
      cwd: node.cwd,
      shell: node.shell,
      scroll_offset: node.scrollOffset,
      session_lost: node.sessionLost,
    };
  }
  return {
    kind: "split",
    id: node.id,
    direction: node.direction,
    ratio: node.ratio,
    first: toLayoutNode(node.first),
    second: toLayoutNode(node.second),
  };
}

export function fromWorkspaceLayout(layout: WorkspaceLayout): LoadedWorkspace {
  const tabs = layout.tabs.map(fromTabLayout);
  const activeTabId =
    layout.active_tab_id && tabs.some((tab) => tab.id === layout.active_tab_id)
      ? layout.active_tab_id
      : (tabs[0]?.id ?? null);
  return { tabs, activeTabId };
}

function fromTabLayout(tab: TerminalTabLayout): TerminalTab {
  return {
    id: tab.id,
    title: tab.title,
    root: fromLayoutNode(tab.root),
    activePaneId: tab.active_pane_id,
    zoomedPaneId: tab.zoomed_pane_id,
  };
}

function fromLayoutNode(node: TerminalLayoutNode): LayoutNode {
  if (node.kind === "leaf") {
    return {
      kind: "leaf",
      id: node.id,
      title: node.title,
      sessionId: node.session_id,
      historyId: node.history_id,
      cwd: node.cwd,
      shell: node.shell,
      scrollOffset: node.scroll_offset,
      sessionLost: node.session_lost,
    };
  }
  return {
    kind: "split",
    id: node.id,
    direction: node.direction,
    ratio: node.ratio,
    first: fromLayoutNode(node.first),
    second: fromLayoutNode(node.second),
  };
}

export function markSavedSessionsLost(tabs: TerminalTab[]): TerminalTab[] {
  return tabs.map((tab) => ({
    ...tab,
    root: markNodeLost(tab.root),
  }));
}

function markNodeLost(node: LayoutNode): LayoutNode {
  if (node.kind === "leaf") {
    return node.sessionId ? { ...node, sessionLost: true } : node;
  }
  return {
    ...node,
    first: markNodeLost(node.first),
    second: markNodeLost(node.second),
  };
}
