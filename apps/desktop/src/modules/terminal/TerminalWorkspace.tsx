/** Tabbed terminal workspace with split panes, zoom, resize, and tab management. */

import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";

import type { TerminalSpawnResponse } from "@muxyard/contracts";

import { isEditableTarget } from "../../app/keymap";
import { trapTabKey, useDialogFocus } from "../../ui/dialogFocus";
import { TerminalIcon } from "./TerminalIcon";
import { TerminalMenu, type TerminalMenuAction } from "./TerminalMenu";
import { TerminalPane } from "./TerminalPane";
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
  updateLeaf,
  type LayoutNode,
  type SplitDirection,
  type TerminalTab,
} from "./layout";
import {
  fromWorkspaceLayout,
  markSavedSessionsLost,
  toWorkspaceLayout,
} from "./persistence";
import {
  closeTerminal,
  getWorkspaceLayout,
  saveWorkspaceLayout,
} from "./terminal";

type WorkspaceState = {
  tabs: TerminalTab[];
  activeTabId: string | null;
};

type PendingClose =
  | {
      kind: "pane";
      tabId: string;
      paneId: string;
      title: string;
    }
  | {
      kind: "tab";
      tabId: string;
      title: string;
      paneCount: number;
    }
  | {
      kind: "others";
      tabId: string;
      title: string;
      count: number;
    }
  | {
      kind: "unsplit";
      tabId: string;
      paneId: string;
      title: string;
    };

export type TerminalWorkspaceHandle = {
  newTab: () => void;
  splitActive: (direction: SplitDirection) => void;
  toggleZoom: () => void;
  requestCloseActivePane: () => void;
  requestCloseOtherTabs: () => void;
  moveActiveTab: (delta: 1 | -1) => void;
  nextTab: () => void;
  previousTab: () => void;
  nextPane: () => void;
  previousPane: () => void;
  renameActiveTab: () => void;
};

type TerminalWorkspaceProps = {
  shortcutsEnabled?: boolean;
};

type PaneLayoutProps = {
  node: LayoutNode;
  tab: TerminalTab;
  activeTabId: string | null;
  onFocus: (tabId: string, paneId: string) => void;
  onClose: (tabId: string, paneId: string) => void;
  onToggleZoom: (tabId: string, paneId: string) => void;
  onRatioChange: (tabId: string, splitId: string, ratio: number) => void;
  onSession: (
    tabId: string,
    paneId: string,
    info: TerminalSpawnResponse,
  ) => void;
  onLost: (tabId: string, paneId: string) => void;
  onViewport: (tabId: string, paneId: string, offset: number) => void;
};

function PaneLayout({
  node,
  tab,
  activeTabId,
  onFocus,
  onClose,
  onToggleZoom,
  onRatioChange,
  onSession,
  onLost,
  onViewport,
}: PaneLayoutProps) {
  if (node.kind === "leaf") {
    const zoomHidden = Boolean(
      tab.zoomedPaneId && tab.zoomedPaneId !== node.id,
    );
    const leaves = listLeaves(tab.root);
    const paneIndex = leaves.findIndex((leaf) => leaf.id === node.id);
    const paneTitle =
      leaves.length <= 1
        ? tab.title
        : `${tab.title} · ${Math.max(paneIndex, 0) + 1}`;
    return (
      <TerminalPane
        key={node.id}
        title={paneTitle}
        active={
          tab.id === activeTabId && tab.activePaneId === node.id && !zoomHidden
        }
        zoomed={tab.zoomedPaneId === node.id}
        zoomHidden={zoomHidden}
        onFocus={() => onFocus(tab.id, node.id)}
        onClose={() => onClose(tab.id, node.id)}
        onToggleZoom={() => onToggleZoom(tab.id, node.id)}
        sessionId={node.sessionId}
        sessionLost={node.sessionLost}
        historyId={node.historyId}
        cwd={node.cwd}
        shell={node.shell}
        scrollOffset={node.scrollOffset}
        onViewport={(offset) => onViewport(tab.id, node.id, offset)}
        onSession={(info) => onSession(tab.id, node.id, info)}
        onLost={() => onLost(tab.id, node.id)}
      />
    );
  }

  const firstHasZoom =
    !tab.zoomedPaneId || Boolean(findLeaf(node.first, tab.zoomedPaneId));
  const secondHasZoom =
    !tab.zoomedPaneId || Boolean(findLeaf(node.second, tab.zoomedPaneId));

  return (
    <div
      className={`terminal-split terminal-split-${node.direction}`}
      data-testid="terminal-split"
    >
      <div
        className="terminal-split-child"
        data-zoom-hidden={firstHasZoom ? "false" : "true"}
        style={{ flexGrow: node.ratio }}
      >
        <PaneLayout
          node={node.first}
          tab={tab}
          activeTabId={activeTabId}
          onFocus={onFocus}
          onClose={onClose}
          onToggleZoom={onToggleZoom}
          onRatioChange={onRatioChange}
          onSession={onSession}
          onLost={onLost}
          onViewport={onViewport}
        />
      </div>
      <SplitDivider
        splitId={node.id}
        direction={node.direction}
        ratio={node.ratio}
        onRatioChange={(ratio) => onRatioChange(tab.id, node.id, ratio)}
      />
      <div
        className="terminal-split-child"
        data-zoom-hidden={secondHasZoom ? "false" : "true"}
        style={{ flexGrow: 1 - node.ratio }}
      >
        <PaneLayout
          node={node.second}
          tab={tab}
          activeTabId={activeTabId}
          onFocus={onFocus}
          onClose={onClose}
          onToggleZoom={onToggleZoom}
          onRatioChange={onRatioChange}
          onSession={onSession}
          onLost={onLost}
          onViewport={onViewport}
        />
      </div>
    </div>
  );
}

type SplitDividerProps = {
  splitId: string;
  direction: SplitDirection;
  ratio: number;
  onRatioChange: (ratio: number) => void;
};

function SplitDivider({
  splitId,
  direction,
  ratio,
  onRatioChange,
}: SplitDividerProps) {
  const dividerRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  const updateFromPointer = (clientX: number, clientY: number) => {
    const parent = dividerRef.current?.parentElement;
    if (!parent) return;
    const rect = parent.getBoundingClientRect();
    const next =
      direction === "horizontal"
        ? (clientX - rect.left) / Math.max(rect.width, 1)
        : (clientY - rect.top) / Math.max(rect.height, 1);
    onRatioChange(next);
  };

  const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(true);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    updateFromPointer(event.clientX, event.clientY);
  };

  const handlePointerEnd = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const step = 0.05;
    let next: number | null = null;
    if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      next = ratio - step;
    } else if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      next = ratio + step;
    } else if (event.key === "Home") {
      next = 0.15;
    } else if (event.key === "End") {
      next = 0.85;
    }
    if (next === null) return;
    event.preventDefault();
    onRatioChange(next);
  };

  return (
    <div
      ref={dividerRef}
      className={`terminal-split-divider terminal-split-divider-${direction}`}
      data-testid="terminal-split-divider"
      data-split-id={splitId}
      data-dragging={dragging ? "true" : "false"}
      role="separator"
      aria-label="Resize terminal split"
      aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
      aria-valuemin={15}
      aria-valuemax={85}
      aria-valuenow={Math.round(ratio * 100)}
      tabIndex={0}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerEnd}
      onPointerCancel={handlePointerEnd}
      onDoubleClick={() => onRatioChange(0.5)}
      onKeyDown={handleKeyDown}
    />
  );
}

function removeTab(state: WorkspaceState, tabId: string): WorkspaceState {
  const index = state.tabs.findIndex((tab) => tab.id === tabId);
  const tabs = state.tabs.filter((tab) => tab.id !== tabId);
  if (state.activeTabId !== tabId) return { ...state, tabs };
  const next = tabs[Math.min(index, tabs.length - 1)] ?? tabs[0] ?? null;
  return { tabs, activeTabId: next?.id ?? null };
}

function createInitialWorkspace(): WorkspaceState {
  const tab = createTab(1);
  return { tabs: [tab], activeTabId: tab.id };
}

function closeTabSessions(tab: TerminalTab) {
  for (const leaf of listLeaves(tab.root)) {
    if (leaf.sessionId) void closeTerminal(leaf.sessionId).catch(() => {});
  }
}

export const TerminalWorkspace = forwardRef<
  TerminalWorkspaceHandle,
  TerminalWorkspaceProps
>(function TerminalWorkspace({ shortcutsEnabled = true }, ref) {
  const [workspace, setWorkspace] = useState<WorkspaceState>({
    tabs: [],
    activeTabId: null,
  });
  const [loaded, setLoaded] = useState(false);
  const [immediateSave, setImmediateSave] = useState(0);
  const [pendingClose, setPendingClose] = useState<PendingClose | null>(null);
  const [editingTabId, setEditingTabId] = useState<string | null>(null);
  const [tabDraft, setTabDraft] = useState("");
  const [draggedTabId, setDraggedTabId] = useState<string | null>(null);
  const nextNumber = useRef(2);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const renameCancelled = useRef(false);
  const activeTab =
    workspace.tabs.find((tab) => tab.id === workspace.activeTabId) ?? null;

  useEffect(() => {
    let cancelled = false;
    void getWorkspaceLayout()
      .then((snapshot) => {
        if (cancelled) return;
        if (!snapshot.layout) {
          setWorkspace(createInitialWorkspace());
          setLoaded(true);
          return;
        }
        const loadedLayout = fromWorkspaceLayout(snapshot.layout);
        const hostMatches =
          snapshot.layout_host_instance_id === snapshot.host_instance_id &&
          snapshot.layout_process_start === snapshot.process_start;
        const tabs = hostMatches
          ? loadedLayout.tabs
          : markSavedSessionsLost(loadedLayout.tabs);
        nextNumber.current = Math.max(2, tabs.length + 1);
        setWorkspace({ tabs, activeTabId: loadedLayout.activeTabId });
        setLoaded(true);
      })
      .catch(() => {
        if (cancelled) return;
        setWorkspace(createInitialWorkspace());
        setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!loaded) return;
    void saveWorkspaceLayout(
      toWorkspaceLayout(workspace.tabs, workspace.activeTabId),
    ).catch(() => {});
  }, [loaded]);

  useEffect(() => {
    if (!loaded || immediateSave === 0) return;
    void saveWorkspaceLayout(
      toWorkspaceLayout(workspace.tabs, workspace.activeTabId),
    ).catch(() => {});
  }, [immediateSave, loaded]);

  useEffect(() => {
    if (!loaded) return;
    const timer = setTimeout(() => {
      void saveWorkspaceLayout(
        toWorkspaceLayout(workspace.tabs, workspace.activeTabId),
      ).catch(() => {});
    }, 200);
    return () => clearTimeout(timer);
  }, [workspace, loaded]);

  useEffect(() => {
    if (!editingTabId) return;
    renameInputRef.current?.focus();
    renameInputRef.current?.select();
  }, [editingTabId]);

  const updateTab = (
    tabId: string,
    update: (tab: TerminalTab) => TerminalTab,
  ) => {
    setWorkspace((previous) => {
      let changed = false;
      const tabs = previous.tabs.map((tab) => {
        if (tab.id !== tabId) return tab;
        const next = update(tab);
        changed = changed || next !== tab;
        return next;
      });
      return changed ? { ...previous, tabs } : previous;
    });
  };

  const newTab = () => {
    const tab = createTab(nextNumber.current);
    nextNumber.current += 1;
    setWorkspace((previous) => ({
      tabs: [...previous.tabs, tab],
      activeTabId: tab.id,
    }));
  };

  const activateTab = (tabId: string) => {
    setWorkspace((previous) =>
      previous.tabs.some((tab) => tab.id === tabId)
        ? { ...previous, activeTabId: tabId }
        : previous,
    );
  };

  const activateTabOffset = (delta: 1 | -1) => {
    setWorkspace((previous) => {
      if (previous.tabs.length < 2) return previous;
      const index = previous.tabs.findIndex(
        (tab) => tab.id === previous.activeTabId,
      );
      const nextIndex =
        (Math.max(index, 0) + delta + previous.tabs.length) %
        previous.tabs.length;
      return {
        ...previous,
        activeTabId: previous.tabs[nextIndex]?.id ?? previous.activeTabId,
      };
    });
  };

  const moveTabBefore = (sourceId: string, targetId: string) => {
    setWorkspace((previous) => {
      const from = previous.tabs.findIndex((tab) => tab.id === sourceId);
      const to = previous.tabs.findIndex((tab) => tab.id === targetId);
      if (from < 0 || to < 0 || from === to) return previous;
      const tabs = [...previous.tabs];
      const [moved] = tabs.splice(from, 1);
      if (!moved) return previous;
      tabs.splice(from < to ? to - 1 : to, 0, moved);
      return { ...previous, tabs };
    });
  };

  const moveActiveTab = (delta: 1 | -1) => {
    if (!activeTab) return;
    setWorkspace((previous) => {
      const from = previous.tabs.findIndex((tab) => tab.id === activeTab.id);
      const to = from + delta;
      if (from < 0 || to < 0 || to >= previous.tabs.length) return previous;
      const tabs = [...previous.tabs];
      const [moved] = tabs.splice(from, 1);
      if (!moved) return previous;
      tabs.splice(to, 0, moved);
      return { ...previous, tabs };
    });
  };

  const activatePane = (tabId: string, paneId: string) => {
    updateTab(tabId, (tab) => {
      if (!findLeaf(tab.root, paneId)) return tab;
      const zoomedPaneId =
        tab.zoomedPaneId && tab.zoomedPaneId !== paneId
          ? null
          : tab.zoomedPaneId;
      if (tab.activePaneId === paneId && tab.zoomedPaneId === zoomedPaneId) {
        return tab;
      }
      return { ...tab, activePaneId: paneId, zoomedPaneId };
    });
  };

  const activatePaneOffset = (delta: 1 | -1) => {
    if (!activeTab) return;
    const paneId = nextPaneId(activeTab.root, activeTab.activePaneId, delta);
    activatePane(activeTab.id, paneId);
  };

  const toggleZoom = () => {
    if (!activeTab) return;
    updateTab(activeTab.id, (tab) => ({
      ...tab,
      zoomedPaneId:
        tab.zoomedPaneId === tab.activePaneId ? null : tab.activePaneId,
    }));
  };

  const togglePaneZoom = (tabId: string, paneId: string) => {
    updateTab(tabId, (tab) => ({
      ...tab,
      activePaneId: paneId,
      zoomedPaneId: tab.zoomedPaneId === paneId ? null : paneId,
    }));
  };

  const splitActive = (direction: SplitDirection) => {
    if (!activeTab) return;
    const newLeaf = createLeaf(activeTab.title);
    const root = splitNode(
      activeTab.root,
      activeTab.activePaneId,
      newLeaf,
      direction,
    );
    if (root === activeTab.root) return;
    updateTab(activeTab.id, (tab) => ({
      ...tab,
      root,
      activePaneId: newLeaf.id,
      zoomedPaneId: null,
    }));
  };

  const updateSplitRatio = (tabId: string, splitId: string, ratio: number) => {
    updateTab(tabId, (tab) => ({
      ...tab,
      root: setSplitRatio(tab.root, splitId, ratio),
    }));
  };

  const updatePaneSession = (
    tabId: string,
    paneId: string,
    info: TerminalSpawnResponse,
  ) => {
    updateTab(tabId, (tab) => ({
      ...tab,
      root: updateLeaf(tab.root, paneId, (leaf) => ({
        ...leaf,
        sessionId: info.stream_id,
        cwd: info.cwd,
        shell: info.shell,
        scrollOffset: 0,
        sessionLost: false,
      })),
    }));
    setImmediateSave((current) => current + 1);
  };

  const markPaneLost = (tabId: string, paneId: string) => {
    updateTab(tabId, (tab) => ({
      ...tab,
      root: updateLeaf(tab.root, paneId, (leaf) => ({
        ...leaf,
        sessionLost: true,
      })),
    }));
    setImmediateSave((current) => current + 1);
  };

  const updatePaneViewport = (
    tabId: string,
    paneId: string,
    offset: number,
  ) => {
    updateTab(tabId, (tab) => ({
      ...tab,
      root: updateLeaf(tab.root, paneId, (leaf) =>
        leaf.scrollOffset === offset ? leaf : { ...leaf, scrollOffset: offset },
      ),
    }));
  };

  const requestClosePane = (tabId: string, paneId: string) => {
    const tab = workspace.tabs.find((candidate) => candidate.id === tabId);
    if (!tab) return;
    if (paneCount(tab.root) <= 1) {
      setPendingClose({
        kind: "tab",
        tabId: tab.id,
        title: tab.title,
        paneCount: 1,
      });
      return;
    }
    const leaf = findLeaf(tab.root, paneId);
    if (!leaf) return;
    const leaves = listLeaves(tab.root);
    const index = leaves.findIndex((candidate) => candidate.id === paneId);
    setPendingClose({
      kind: "pane",
      tabId: tab.id,
      paneId,
      title:
        leaves.length <= 1
          ? tab.title
          : `${tab.title} · ${Math.max(index, 0) + 1}`,
    });
  };

  const requestCloseTab = (tabId: string) => {
    const tab = workspace.tabs.find((candidate) => candidate.id === tabId);
    if (!tab) return;
    setPendingClose({
      kind: "tab",
      tabId: tab.id,
      title: tab.title,
      paneCount: paneCount(tab.root),
    });
  };

  const requestCloseActivePane = () => {
    if (!activeTab) return;
    requestClosePane(activeTab.id, activeTab.activePaneId);
  };

  const requestUnsplit = () => {
    if (!activeTab || paneCount(activeTab.root) <= 1) return;
    const leaves = listLeaves(activeTab.root);
    const index = leaves.findIndex(
      (leaf) => leaf.id === activeTab.activePaneId,
    );
    setPendingClose({
      kind: "unsplit",
      tabId: activeTab.id,
      paneId: activeTab.activePaneId,
      title: `${activeTab.title} · ${Math.max(index, 0) + 1}`,
    });
  };

  const requestCloseOtherTabs = () => {
    if (!activeTab || workspace.tabs.length < 2) return;
    setPendingClose({
      kind: "others",
      tabId: activeTab.id,
      title: activeTab.title,
      count: workspace.tabs.length - 1,
    });
  };

  const confirmClose = () => {
    const close = pendingClose;
    if (!close) return;
    setPendingClose(null);
    const closingTab = workspace.tabs.find((tab) => tab.id === close.tabId);
    if (close.kind === "tab" && closingTab) {
      closeTabSessions(closingTab);
    } else if (close.kind === "others") {
      for (const tab of workspace.tabs) {
        if (tab.id !== close.tabId) closeTabSessions(tab);
      }
    } else if (
      closingTab &&
      (close.kind === "pane" || close.kind === "unsplit")
    ) {
      const leaf = findLeaf(closingTab.root, close.paneId);
      if (leaf?.sessionId) void closeTerminal(leaf.sessionId).catch(() => {});
    }
    setWorkspace((previous) => {
      if (close.kind === "others") {
        const tabs = previous.tabs.filter((tab) => tab.id === close.tabId);
        return tabs.length ? { tabs, activeTabId: close.tabId } : previous;
      }
      if (close.kind === "tab") return removeTab(previous, close.tabId);
      const tab = previous.tabs.find(
        (candidate) => candidate.id === close.tabId,
      );
      if (!tab) return previous;
      const before = listLeaves(tab.root);
      const root = closeNode(tab.root, close.paneId);
      if (!root) return removeTab(previous, tab.id);
      const leaves = listLeaves(root);
      let activePaneId = tab.activePaneId;
      if (!leaves.some((leaf) => leaf.id === activePaneId)) {
        const closedIndex = before.findIndex(
          (leaf) => leaf.id === close.paneId,
        );
        activePaneId =
          leaves[Math.min(Math.max(closedIndex, 0), leaves.length - 1)]?.id ??
          leaves[0]?.id ??
          activePaneId;
      }
      return {
        ...previous,
        tabs: previous.tabs.map((candidate) =>
          candidate.id === tab.id
            ? {
                ...candidate,
                root,
                activePaneId,
                zoomedPaneId: null,
              }
            : candidate,
        ),
      };
    });
  };

  const beginRename = (tab: TerminalTab) => {
    renameCancelled.current = false;
    setEditingTabId(tab.id);
    setTabDraft(tab.title);
  };

  const commitRename = (tabId: string, value: string) => {
    const title = value.trim();
    setWorkspace((previous) => ({
      ...previous,
      tabs: previous.tabs.map((tab) =>
        tab.id === tabId ? { ...tab, title: title || tab.title } : tab,
      ),
    }));
    setEditingTabId(null);
    setTabDraft("");
  };

  const cancelRename = () => {
    renameCancelled.current = true;
    setEditingTabId(null);
    setTabDraft("");
  };

  const handleWorkspaceKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (!shortcutsEnabled || pendingClose) return;
    if (event.repeat || event.altKey) return;
    if (!(event.metaKey || event.ctrlKey) || !event.shiftKey) return;
    if (isEditableTarget(event.target)) return;

    const key = event.key;
    const lower = key.toLowerCase();
    let handled = true;
    if (lower === "t") {
      newTab();
    } else if (lower === "w") {
      requestCloseActivePane();
    } else if (lower === "d") {
      splitActive("horizontal");
    } else if (lower === "e") {
      splitActive("vertical");
    } else if (lower === "z") {
      toggleZoom();
    } else if (key === "]" || key === "}") {
      activateTabOffset(1);
    } else if (key === "[" || key === "{") {
      activateTabOffset(-1);
    } else if (key === "ArrowRight" || key === "ArrowDown") {
      activatePaneOffset(1);
    } else if (key === "ArrowLeft" || key === "ArrowUp") {
      activatePaneOffset(-1);
    } else {
      handled = false;
    }
    if (!handled) return;
    event.preventDefault();
    event.stopPropagation();
  };

  const handleTabListKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (editingTabId) return;
    if (!(event.target instanceof Element)) return;
    if (!event.target.closest('[role="tab"]')) return;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      activateTabOffset(1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      activateTabOffset(-1);
    } else if (event.key === "Home") {
      event.preventDefault();
      activateTab(workspace.tabs[0]?.id ?? "");
    } else if (event.key === "End") {
      event.preventDefault();
      activateTab(workspace.tabs.at(-1)?.id ?? "");
    }
  };

  useImperativeHandle(ref, () => ({
    newTab,
    splitActive,
    toggleZoom,
    requestCloseActivePane,
    requestCloseOtherTabs,
    moveActiveTab,
    nextTab: () => activateTabOffset(1),
    previousTab: () => activateTabOffset(-1),
    nextPane: () => activatePaneOffset(1),
    previousPane: () => activatePaneOffset(-1),
    renameActiveTab: () => {
      if (activeTab) beginRename(activeTab);
    },
  }));

  const menuActions: TerminalMenuAction[] = [
    {
      id: "new-tab",
      label: "New tab",
      icon: "plus",
      onSelect: newTab,
    },
    {
      id: "split-right",
      label: "Split right",
      icon: "split-right",
      disabled: !activeTab,
      onSelect: () => splitActive("horizontal"),
    },
    {
      id: "split-down",
      label: "Split down",
      icon: "split-down",
      disabled: !activeTab,
      onSelect: () => splitActive("vertical"),
    },
    {
      id: "toggle-zoom",
      label: activeTab?.zoomedPaneId ? "Restore pane" : "Maximize pane",
      icon: activeTab?.zoomedPaneId ? "restore" : "maximize",
      disabled: !activeTab,
      onSelect: toggleZoom,
    },
    {
      id: "close-pane",
      label: "Close active pane",
      icon: "close",
      disabled: !activeTab,
      danger: true,
      onSelect: requestCloseActivePane,
    },
    {
      id: "unsplit",
      label: "Unsplit tab",
      disabled: !activeTab || paneCount(activeTab.root) <= 1,
      danger: true,
      onSelect: requestUnsplit,
    },
    {
      id: "close-others",
      label: "Close other tabs",
      disabled: !activeTab || workspace.tabs.length < 2,
      danger: true,
      onSelect: requestCloseOtherTabs,
    },
    {
      id: "rename-tab",
      label: "Rename active tab",
      disabled: !activeTab,
      onSelect: () => {
        if (activeTab) beginRename(activeTab);
      },
    },
    {
      id: "move-tab-left",
      label: "Move tab left",
      disabled: !activeTab,
      onSelect: () => moveActiveTab(-1),
    },
    {
      id: "move-tab-right",
      label: "Move tab right",
      disabled: !activeTab,
      onSelect: () => moveActiveTab(1),
    },
  ];

  if (!loaded) {
    return (
      <section className="terminal-workspace" aria-label="Terminal workspace">
        <div className="terminal-empty">
          <p>Restoring terminal layout…</p>
        </div>
      </section>
    );
  }

  return (
    <section
      className="terminal-workspace"
      aria-label="Terminal workspace"
      data-zoomed={activeTab?.zoomedPaneId ? "true" : "false"}
      onKeyDownCapture={handleWorkspaceKeyDown}
    >
      <header className="terminal-tabbar">
        <div
          className="terminal-tab-list"
          role="tablist"
          aria-label="Terminal tabs"
          onKeyDown={handleTabListKeyDown}
        >
          {workspace.tabs.map((tab) => {
            const editing = editingTabId === tab.id;
            return (
              <div
                className="terminal-tab"
                role="presentation"
                data-active={
                  tab.id === workspace.activeTabId ? "true" : "false"
                }
                draggable={!editing}
                key={tab.id}
                onDragStart={(event) => {
                  if (editing) return;
                  setDraggedTabId(tab.id);
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData("text/plain", tab.id);
                }}
                onDragOver={(event) => {
                  if (!draggedTabId) return;
                  event.preventDefault();
                  event.dataTransfer.dropEffect = "move";
                }}
                onDrop={(event) => {
                  event.preventDefault();
                  if (draggedTabId) moveTabBefore(draggedTabId, tab.id);
                  setDraggedTabId(null);
                }}
                onDragEnd={() => setDraggedTabId(null)}
              >
                {editing ? (
                  <input
                    ref={renameInputRef}
                    className="terminal-tab-rename"
                    aria-label={`Rename ${tab.title}`}
                    value={tabDraft}
                    onChange={(event) => setTabDraft(event.target.value)}
                    onBlur={() => {
                      if (renameCancelled.current) {
                        renameCancelled.current = false;
                        return;
                      }
                      commitRename(tab.id, tabDraft);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        commitRename(tab.id, tabDraft);
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        cancelRename();
                      }
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    role="tab"
                    id={`terminal-tab-${tab.id}`}
                    aria-selected={tab.id === workspace.activeTabId}
                    aria-controls={`terminal-panel-${tab.id}`}
                    className="terminal-tab-button"
                    title="Double-click to rename"
                    onClick={() => activateTab(tab.id)}
                    onDoubleClick={() => beginRename(tab)}
                  >
                    {tab.title}
                    {tab.zoomedPaneId ? (
                      <span className="terminal-tab-zoom" title="Maximized">
                        <TerminalIcon name="maximize" />
                      </span>
                    ) : null}
                  </button>
                )}
                <button
                  type="button"
                  className="terminal-tab-close"
                  aria-label={`Close tab ${tab.title}`}
                  title={`Close tab ${tab.title}`}
                  onClick={() => requestCloseTab(tab.id)}
                >
                  <TerminalIcon name="close" />
                </button>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          className="terminal-icon-button terminal-new-tab"
          aria-label="New tab"
          title="New tab"
          onClick={newTab}
        >
          <TerminalIcon name="plus" />
        </button>
        <TerminalMenu actions={menuActions} />
      </header>

      <div className="terminal-tab-panels">
        {workspace.tabs.map((tab) => (
          <div
            key={tab.id}
            id={`terminal-panel-${tab.id}`}
            className="terminal-tab-panel"
            role="tabpanel"
            aria-labelledby={`terminal-tab-${tab.id}`}
            hidden={tab.id !== workspace.activeTabId}
          >
            <PaneLayout
              node={tab.root}
              tab={tab}
              activeTabId={workspace.activeTabId}
              onFocus={activatePane}
              onClose={requestClosePane}
              onToggleZoom={togglePaneZoom}
              onRatioChange={updateSplitRatio}
              onSession={updatePaneSession}
              onLost={markPaneLost}
              onViewport={updatePaneViewport}
            />
          </div>
        ))}
        {workspace.tabs.length === 0 ? (
          <div className="terminal-empty">
            <p>No terminal tabs open.</p>
            <button type="button" className="primary-button" onClick={newTab}>
              New terminal
            </button>
          </div>
        ) : null}
      </div>

      {pendingClose ? (
        <CloseTerminalDialog
          close={pendingClose}
          onCancel={() => setPendingClose(null)}
          onConfirm={confirmClose}
        />
      ) : null}
    </section>
  );
});

TerminalWorkspace.displayName = "TerminalWorkspace";

type CloseTerminalDialogProps = {
  close: PendingClose;
  onCancel: () => void;
  onConfirm: () => void;
};

function CloseTerminalDialog({
  close,
  onCancel,
  onConfirm,
}: CloseTerminalDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  useDialogFocus(true, cancelRef);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    trapTabKey(event, dialogRef.current);
  };

  const handleBackdrop = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onCancel();
  };

  const title =
    close.kind === "pane"
      ? "Close terminal pane?"
      : close.kind === "tab"
        ? "Close terminal tab?"
        : close.kind === "unsplit"
          ? "Unsplit terminal tab?"
          : "Close other terminal tabs?";
  const detail =
    close.kind === "pane"
      ? `Close ${close.title}? The running shell and its process will be terminated.`
      : close.kind === "tab"
        ? `Close ${close.title} and its ${close.paneCount} ${
            close.paneCount === 1 ? "pane" : "panes"
          }? The running shells and processes will be terminated.`
        : close.kind === "unsplit"
          ? `Close ${close.title} and collapse the split? The other panes in this tab stay open.`
          : `Close the other ${close.count} ${
              close.count === 1 ? "tab" : "tabs"
            }? The running shells and processes will be terminated.`;
  const confirmLabel =
    close.kind === "pane"
      ? "Close pane"
      : close.kind === "tab"
        ? "Close tab"
        : close.kind === "unsplit"
          ? "Unsplit tab"
          : "Close other tabs";

  return (
    <div className="overlay-backdrop" onMouseDown={handleBackdrop}>
      <div
        ref={dialogRef}
        className="terminal-close-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="terminal-close-title"
        aria-describedby="terminal-close-detail"
        onKeyDown={handleKeyDown}
      >
        <p className="eyebrow">Confirm close</p>
        <h2 id="terminal-close-title">{title}</h2>
        <p id="terminal-close-detail">{detail}</p>
        <div className="terminal-close-actions">
          <button
            ref={cancelRef}
            type="button"
            className="secondary-button"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button type="button" className="danger-button" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
