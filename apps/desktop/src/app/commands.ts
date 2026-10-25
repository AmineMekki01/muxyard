/** Local-only command registry for layout and focus actions. */

import type { FocusRegion } from "./keymap";

export type CommandGroup = "Layout" | "Focus" | "Help";

export type CommandContext = {
  toggleSidebar: () => void;
  toggleInspector: () => void;
  focusRegion: (region: FocusRegion) => void;
  openHelp: () => void;
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
