// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { AppShell } from "./AppShell";

(
  globalThis as typeof globalThis & {
    IS_REACT_ACT_ENVIRONMENT?: boolean;
  }
).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  document.body.replaceChildren();
});

async function renderApp() {
  await act(async () => root.render(<AppShell />));
}

function get<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element: ${selector}`);
  return element;
}

function press(target: EventTarget, init: KeyboardEventInit): boolean {
  return target.dispatchEvent(
    new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ...init,
    }),
  );
}

async function setInput(input: HTMLInputElement, value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )?.set;
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("shell keyboard behavior", () => {
  it("opens the palette by keyboard and focuses its search field", async () => {
    await renderApp();
    await act(async () => {
      expect(press(window, { key: "P", ctrlKey: true, shiftKey: true })).toBe(
        false,
      );
    });
    const input = get<HTMLInputElement>("#command-palette-input");
    expect(document.activeElement).toBe(input);
    expect(input.getAttribute("role")).toBe("combobox");
    expect(get('[data-testid="command-palette"]')).toBeTruthy();
  });

  it("filters commands and runs the selected local action", async () => {
    await renderApp();
    await act(async () => {
      press(window, { key: "P", ctrlKey: true, shiftKey: true });
    });
    const input = get<HTMLInputElement>("#command-palette-input");
    await setInput(input, "sidebar");
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2);
    await act(async () => {
      press(input, { key: "Enter" });
    });
    expect(document.querySelector("#command-palette-input")).toBeNull();
    expect(document.querySelector("#workspace-sidebar")).toBeNull();
    expect(
      get<HTMLButtonElement>('[data-testid="toggle-sidebar"]').getAttribute(
        "aria-expanded",
      ),
    ).toBe("false");
  });

  it("closes on Escape and restores the previous focus", async () => {
    await renderApp();
    const trigger = get<HTMLButtonElement>(
      '[data-testid="command-palette-trigger"]',
    );
    trigger.focus();
    await act(async () => {
      trigger.click();
    });
    const input = get<HTMLInputElement>("#command-palette-input");
    expect(document.activeElement).toBe(input);
    await act(async () => {
      press(input, { key: "Escape" });
    });
    expect(document.querySelector("#command-palette-input")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("does not intercept terminal Ctrl+C, Ctrl+D, Escape, or palette keys", async () => {
    await renderApp();
    const terminal = document.createElement("div");
    terminal.dataset.terminalFocus = "true";
    terminal.tabIndex = 0;
    document.body.append(terminal);
    terminal.focus();

    await act(async () => {
      expect(press(terminal, { key: "c", ctrlKey: true })).toBe(true);
      expect(press(terminal, { key: "d", ctrlKey: true })).toBe(true);
      expect(press(terminal, { key: "Escape" })).toBe(true);
      expect(press(terminal, { key: "P", ctrlKey: true, shiftKey: true })).toBe(
        true,
      );
      expect(press(terminal, { key: "?", shiftKey: true })).toBe(true);
    });

    const editor = document.createElement("input");
    document.body.append(editor);
    editor.focus();
    await act(async () => {
      expect(press(editor, { key: "?", shiftKey: true })).toBe(true);
      expect(press(editor, { key: "P", ctrlKey: true, shiftKey: true })).toBe(
        true,
      );
    });

    expect(document.querySelector("#command-palette-input")).toBeNull();
    expect(document.querySelector('[data-testid="shortcut-help"]')).toBeNull();
  });

  it("opens the shortcut reference with ? and closes it with Escape", async () => {
    await renderApp();
    await act(async () => {
      expect(press(window, { key: "?", shiftKey: true })).toBe(false);
    });
    const close = get<HTMLButtonElement>(
      '[aria-label="Close keyboard shortcuts"]',
    );
    expect(document.activeElement).toBe(close);
    await act(async () => {
      press(close, { key: "Escape" });
    });
    expect(document.querySelector('[data-testid="shortcut-help"]')).toBeNull();
  });

  it("does not run a command when Enter activates a dialog button", async () => {
    await renderApp();
    await act(async () => {
      press(window, { key: "P", ctrlKey: true, shiftKey: true });
    });
    const close = get<HTMLButtonElement>(
      '[aria-label="Close command palette"]',
    );
    close.focus();
    await act(async () => {
      press(close, { key: "Enter" });
    });
    expect(document.querySelector("#command-palette-input")).not.toBeNull();
    expect(document.querySelector("#workspace-sidebar")).not.toBeNull();
    await act(async () => close.click());
    expect(document.querySelector("#command-palette-input")).toBeNull();
  });

  it("toggles both side panels and exposes their state", async () => {
    await renderApp();
    const sidebar = get<HTMLButtonElement>('[data-testid="toggle-sidebar"]');
    const inspector = get<HTMLButtonElement>(
      '[data-testid="toggle-inspector"]',
    );

    await act(async () => sidebar.click());
    expect(document.querySelector("#workspace-sidebar")).toBeNull();
    expect(sidebar.getAttribute("aria-expanded")).toBe("false");
    await act(async () => sidebar.click());
    expect(document.querySelector("#workspace-sidebar")).not.toBeNull();

    await act(async () => inspector.click());
    expect(document.querySelector("#workspace-inspector")).toBeNull();
    expect(inspector.getAttribute("aria-expanded")).toBe("false");
    await act(async () => inspector.click());
    expect(document.querySelector("#workspace-inspector")).not.toBeNull();
  });

  it("does nothing privileged for an unknown palette query", async () => {
    await renderApp();
    await act(async () => {
      press(window, { key: "P", ctrlKey: true, shiftKey: true });
    });
    const input = get<HTMLInputElement>("#command-palette-input");
    await setInput(input, "rm -rf /");
    expect(document.querySelector(".command-empty")).not.toBeNull();
    await act(async () => {
      press(input, { key: "Enter" });
    });
    expect(document.querySelector("#command-palette-input")).not.toBeNull();
    expect(document.querySelector("#workspace-sidebar")).not.toBeNull();
  });

  it("gives every shell button an accessible name", async () => {
    await renderApp();
    for (const button of document.querySelectorAll<HTMLButtonElement>(
      "button",
    )) {
      const name =
        button.getAttribute("aria-label")?.trim() || button.textContent?.trim();
      expect(name).toBeTruthy();
    }
  });
});
