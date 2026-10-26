/** Local-only command registry for layout and focus actions. */

import type { FocusRegion } from "./keymap";
import type { SplitDirection } from "../modules/terminal/layout";

export type CommandGroup = "Layout" | "Focus" | "Terminal" | "Help";

export type CommandContext = {
  toggleSidebar: () => void;
  toggleInspector: () => void;
  focusRegion: (region: FocusRegion) => void;
  openHelp: () => void;
  newTerminalTab: () => void;
  splitTerminal: (direction: SplitDirection) => void;
  toggleTerminalZoom: () => void;
  closeTerminalPane: () => void;
  closeOtherTerminalTabs: () => void;
  moveActiveTerminalTab: (delta: 1 | -1) => void;
  nextTerminalTab: () => void;
  previousTerminalTab: () => void;
  nextTerminalPane: () => void;
  previousTerminalPane: () => void;
  renameActiveTerminalTab: () => void;
};

export type AppCommand = {
  id: string;
  label: string;
  description: string;
  group: CommandGroup;
  keywords: readonly string[];
  run: (context: CommandContext) => void;
};

export function createCommands(): readonly AppCommand[] {
  return [
    {
      id: "layout.toggle-sidebar",
      label: "Toggle sidebar",
      description: "Show or hide the projects, tasks, and attention sidebar.",
      group: "Layout",
      keywords: ["left", "panel", "projects", "tasks", "attention"],
      run: (context) => context.toggleSidebar(),
    },
    {
      id: "layout.toggle-inspector",
      label: "Toggle inspector",
      description: "Show or hide changes, verification, and review details.",
      group: "Layout",
      keywords: ["right", "panel", "changes", "verification", "review"],
      run: (context) => context.toggleInspector(),
    },
    {
      id: "focus.sidebar",
      label: "Focus sidebar",
      description: "Move keyboard focus to the workspace sidebar.",
      group: "Focus",
      keywords: ["left", "projects", "tasks", "sessions"],
      run: (context) => context.focusRegion("sidebar"),
    },
    {
      id: "focus.main",
      label: "Focus main workspace",
      description: "Move keyboard focus to the main empty workspace.",
      group: "Focus",
      keywords: ["center", "content", "workspace"],
      run: (context) => context.focusRegion("main"),
    },
    {
      id: "focus.inspector",
      label: "Focus inspector",
      description: "Move keyboard focus to the inspector.",
      group: "Focus",
      keywords: ["right", "changes", "verification"],
      run: (context) => context.focusRegion("inspector"),
    },
    {
      id: "terminal.new-tab",
      label: "New terminal tab",
      description: "Create a fresh shell in a new terminal tab.",
      group: "Terminal",
      keywords: ["shell", "tab", "new", "terminal"],
      run: (context) => context.newTerminalTab(),
    },
    {
      id: "terminal.split-right",
      label: "Split terminal right",
      description: "Place a new shell beside the active pane.",
      group: "Terminal",
      keywords: ["split", "horizontal", "right", "pane"],
      run: (context) => context.splitTerminal("horizontal"),
    },
    {
      id: "terminal.split-down",
      label: "Split terminal down",
      description: "Place a new shell below the active pane.",
      group: "Terminal",
      keywords: ["split", "vertical", "down", "pane"],
      run: (context) => context.splitTerminal("vertical"),
    },
    {
      id: "terminal.toggle-zoom",
      label: "Maximize or restore terminal pane",
      description: "Show the active pane alone or restore the split layout.",
      group: "Terminal",
      keywords: ["maximize", "zoom", "restore", "pane"],
      run: (context) => context.toggleTerminalZoom(),
    },
    {
      id: "terminal.close-pane",
      label: "Close active terminal pane",
      description: "Ask before terminating the active pane's shell.",
      group: "Terminal",
      keywords: ["close", "kill", "pane", "shell"],
      run: (context) => context.closeTerminalPane(),
    },
    {
      id: "terminal.close-other-tabs",
      label: "Close other terminal tabs",
      description: "Ask before terminating every tab except the active one.",
      group: "Terminal",
      keywords: ["close", "other", "tabs", "shells"],
      run: (context) => context.closeOtherTerminalTabs(),
    },
    {
      id: "terminal.rename-tab",
      label: "Rename active terminal tab",
      description: "Edit the active tab title.",
      group: "Terminal",
      keywords: ["rename", "title", "tab"],
      run: (context) => context.renameActiveTerminalTab(),
    },
    {
      id: "terminal.move-tab-left",
      label: "Move terminal tab left",
      description: "Move the active tab one position left.",
      group: "Terminal",
      keywords: ["move", "reorder", "tab", "left"],
      run: (context) => context.moveActiveTerminalTab(-1),
    },
    {
      id: "terminal.move-tab-right",
      label: "Move terminal tab right",
      description: "Move the active tab one position right.",
      group: "Terminal",
      keywords: ["move", "reorder", "tab", "right"],
      run: (context) => context.moveActiveTerminalTab(1),
    },
    {
      id: "terminal.next-tab",
      label: "Next terminal tab",
      description: "Switch to the next terminal tab.",
      group: "Terminal",
      keywords: ["tab", "next", "switch"],
      run: (context) => context.nextTerminalTab(),
    },
    {
      id: "terminal.previous-tab",
      label: "Previous terminal tab",
      description: "Switch to the previous terminal tab.",
      group: "Terminal",
      keywords: ["tab", "previous", "switch"],
      run: (context) => context.previousTerminalTab(),
    },
    {
      id: "terminal.next-pane",
      label: "Next terminal pane",
      description: "Focus the next pane in the active tab.",
      group: "Terminal",
      keywords: ["pane", "next", "focus", "split"],
      run: (context) => context.nextTerminalPane(),
    },
    {
      id: "terminal.previous-pane",
      label: "Previous terminal pane",
      description: "Focus the previous pane in the active tab.",
      group: "Terminal",
      keywords: ["pane", "previous", "focus", "split"],
      run: (context) => context.previousTerminalPane(),
    },
    {
      id: "help.shortcuts",
      label: "Show keyboard shortcuts",
      description: "Open the local keyboard reference.",
      group: "Help",
      keywords: ["keys", "keymap", "help"],
      run: (context) => context.openHelp(),
    },
  ];
}

export function filterCommands(
  commands: readonly AppCommand[],
  query: string,
): readonly AppCommand[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return commands;
  return commands.filter((command) => {
    const haystack = [
      command.label,
      command.description,
      command.group,
      ...command.keywords,
    ]
      .join(" ")
      .toLowerCase();
    return haystack.includes(normalized);
  });
}
