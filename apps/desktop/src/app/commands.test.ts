import { describe, expect, it, vi } from "vitest";

import {
  createCommands,
  filterCommands,
  type CommandContext,
} from "./commands";

function context(): CommandContext {
  return {
    toggleSidebar: vi.fn(),
    toggleInspector: vi.fn(),
    focusRegion: vi.fn(),
    openHelp: vi.fn(),
    newTerminalTab: vi.fn(),
    splitTerminal: vi.fn(),
    toggleTerminalZoom: vi.fn(),
    closeTerminalPane: vi.fn(),
    closeOtherTerminalTabs: vi.fn(),
    moveActiveTerminalTab: vi.fn(),
    nextTerminalTab: vi.fn(),
    previousTerminalTab: vi.fn(),
    nextTerminalPane: vi.fn(),
    previousTerminalPane: vi.fn(),
    renameActiveTerminalTab: vi.fn(),
  };
}

describe("local command registry", () => {
  it("contains only local layout, focus, terminal, and help actions", () => {
    const commands = createCommands();
    expect(commands.length).toBeGreaterThan(0);
    for (const command of commands) {
      expect(command.id).toMatch(/^(layout|focus|terminal|help)\./);
      expect(typeof command.run).toBe("function");
    }
    const ids = commands.map((command) => command.id);
    expect(ids).not.toContain("host.start");
    expect(ids).not.toContain("terminal.write");
    expect(ids).not.toContain("approval.respond");
    expect(ids).not.toContain("git.merge");
  });

  it("filters by label, description, group, and keywords", () => {
    const commands = createCommands();
    expect(
      filterCommands(commands, "sidebar").map((command) => command.id),
    ).toEqual(["layout.toggle-sidebar", "focus.sidebar"]);
    expect(
      filterCommands(commands, "changes").map((command) => command.id),
    ).toContain("layout.toggle-inspector");
    expect(filterCommands(commands, "rm -rf /")).toEqual([]);
    expect(filterCommands(commands, "  ")).toHaveLength(commands.length);
  });

  it("runs only the selected local action", () => {
    const actions = context();
    const commands = createCommands();
    const command = commands.find(
      (candidate) => candidate.id === "layout.toggle-sidebar",
    );
    expect(command).toBeDefined();
    command?.run(actions);
    expect(actions.toggleSidebar).toHaveBeenCalledOnce();
    expect(actions.toggleInspector).not.toHaveBeenCalled();
    expect(actions.focusRegion).not.toHaveBeenCalled();
  });
});
