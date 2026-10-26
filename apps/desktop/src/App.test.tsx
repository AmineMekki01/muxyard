import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("./modules/terminal/TerminalPane", () => ({
  TerminalPane: () => <div>Terminal pane</div>,
}));

import { App } from "./App";

describe("workspace shell", () => {
  it("renders the shell with honest host and empty states", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain("<h1>No project open</h1>");
    expect(html).toContain("Host starting");
    expect(html).toContain("Connecting to local host");
    expect(html).toContain("Remote off");
    expect(html).toContain("No pending attention");
    expect(html).toContain("Inspector hidden");
    expect(html).toContain("Command palette");
    expect(html).toContain("not connected yet");
    expect(html).toContain("local terminal is available");
  });

  it("offers only local shell controls, not pretend terminal surfaces", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain('aria-label="Hide sidebar"');
    expect(html).toContain('aria-label="Show inspector"');
    expect(html).toContain('aria-haspopup="dialog"');
    expect(html).not.toMatch(/<(iframe|canvas)\b/);
    expect(html).not.toMatch(/terminal is ready|agent is running/i);
  });
});
