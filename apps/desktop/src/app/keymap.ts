/** Keyboard routing rules that keep terminal control sequences out of the shell. */

export type FocusRegion = "sidebar" | "main" | "inspector";

export type ShortcutDefinition = {
  keys: string;
  label: string;
  description: string;
};

export const SHORTCUTS: readonly ShortcutDefinition[] = [
  {
    keys: "Cmd/Ctrl+Shift+P",
    label: "Command palette",
    description: "Search local layout and focus actions.",
  },
  {
    keys: "?",
    label: "Keyboard shortcuts",
    description: "Show this list when focus is outside a terminal or editor.",
  },
  {
    keys: "Escape",
    label: "Close overlay",
    description: "Close the command palette or shortcuts panel.",
  },
  {
    keys: "Tab / Shift+Tab",
    label: "Move focus",
    description: "Cycle controls inside an open overlay.",
  },
  {
    keys: "Cmd/Ctrl+Shift+T",
    label: "New terminal tab",
    description: "Create a fresh shell in a new tab.",
  },
  {
    keys: "Cmd/Ctrl+Shift+D / E",
    label: "Split terminal",
    description: "Split the active pane right or down.",
  },
  {
    keys: "Cmd/Ctrl+Shift+W",
    label: "Close active pane",
    description: "Ask before terminating the pane's running shell.",
  },
  {
    keys: "Cmd/Ctrl+Shift+Z",
    label: "Maximize active pane",
    description: "Show the active pane alone, or restore the split layout.",
  },
  {
    keys: "Double-click tab",
    label: "Rename tab",
    description: "Edit the terminal tab title inline.",
  },
  {
    keys: "Cmd/Ctrl+Shift+[ / ]",
    label: "Switch tabs",
    description: "Move to the previous or next terminal tab.",
  },
  {
    keys: "Cmd/Ctrl+Shift+Arrow",
    label: "Switch panes",
    description: "Focus the previous or next pane in the active tab.",
  },
];

export function isPaletteShortcut(
  event: Pick<
    KeyboardEvent,
    "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey"
  >,
): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    event.shiftKey &&
    !event.altKey &&
    event.key.toLowerCase() === "p"
  );
}

export function isHelpShortcut(
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">,
): boolean {
  return !event.metaKey && !event.ctrlKey && !event.altKey && event.key === "?";
}

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest(
      'input, textarea, select, [contenteditable="true"], [contenteditable=""]',
    ),
  );
}

export function isTerminalTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest('[data-terminal-focus="true"], .xterm, [role="terminal"]'),
  );
}

export function shouldIgnoreGlobalShortcut(
  target: EventTarget | null,
): boolean {
  return isEditableTarget(target) || isTerminalTarget(target);
}

export function nextEnabledIndex(
  current: number,
  length: number,
  delta: 1 | -1,
): number {
  if (length === 0) return -1;
  return (current + delta + length) % length;
}
