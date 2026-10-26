// @vitest-environment jsdom

import { act, createRef, type Ref } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({
    title,
    active,
    zoomed,
    zoomHidden,
    onFocus,
    onClose,
  }: {
    title: string;
    active: boolean;
    zoomed?: boolean;
    zoomHidden?: boolean;
    onFocus?: () => void;
    onClose?: () => void;
  }) => (
    <section
      data-testid="pane"
      data-title={title}
      data-active={active ? "true" : "false"}
      data-zoomed={zoomed ? "true" : "false"}
      data-zoom-hidden={zoomHidden ? "true" : "false"}
      onMouseDown={onFocus}
    >
      <span>{title}</span>
      <button type="button" aria-label={`Close ${title}`} onClick={onClose}>
        Close
      </button>
    </section>
  ),
}));

import {
  TerminalWorkspace,
  type TerminalWorkspaceHandle,
} from "./TerminalWorkspace";

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

async function render(
  shortcutsEnabled = true,
  ref?: Ref<TerminalWorkspaceHandle>,
) {
  await act(async () => {
    root.render(
      <TerminalWorkspace ref={ref} shortcutsEnabled={shortcutsEnabled} />,
    );
  });
}

function button(text: string): HTMLButtonElement {
  const match = Array.from(
    document.querySelectorAll<HTMLButtonElement>("button"),
  ).find((candidate) => candidate.textContent?.trim() === text);
  if (!match) throw new Error(`Missing button: ${text}`);
  return match;
}

async function menuAction(label: string) {
  const trigger = document.querySelector<HTMLButtonElement>(
    '[aria-label="Terminal actions"]',
  );
  if (!trigger) throw new Error("Missing terminal actions menu");
  await click(trigger);
  await click(button(label));
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

async function click(target: Element) {
  await act(async () => {
    (target as HTMLElement).click();
  });
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

function dblclick(target: Element) {
  act(() => {
    target.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  });
}

function panes(): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>('[data-testid="pane"]'),
  );
}

function getButton(selector: string): HTMLButtonElement {
  const match = document.querySelector<HTMLButtonElement>(selector);
  if (!match) throw new Error(`Missing button: ${selector}`);
  return match;
}

function tabs(): HTMLButtonElement[] {
  return Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
  );
}

describe("terminal workspace", () => {
  it("creates tabs without unmounting hidden panes", async () => {
    await render();
    expect(tabs()).toHaveLength(1);
    expect(panes()).toHaveLength(1);
    expect(panes()[0]?.dataset.active).toBe("true");
    const firstPane = panes()[0];

    await click(getButton('[aria-label="New tab"]'));
    expect(tabs()).toHaveLength(2);
    expect(panes()).toHaveLength(2);
    expect(panes()[0]).toBe(firstPane);
    const panels = document.querySelectorAll<HTMLElement>('[role="tabpanel"]');
    expect(panels[0]?.hidden).toBe(true);
    expect(panels[1]?.hidden).toBe(false);
    expect(panes()[1]?.dataset.active).toBe("true");

    await click(tabs()[0]!);
    expect(panels[0]?.hidden).toBe(false);
    expect(panels[1]?.hidden).toBe(true);
    expect(panes()[0]?.dataset.active).toBe("true");
  });

  it("splits the active pane and preserves per-tab active panes", async () => {
    await render();
    await menuAction("Split right");
    expect(panes()).toHaveLength(2);
    expect(
      document.querySelectorAll('[data-testid="terminal-split"]'),
    ).toHaveLength(1);
    expect(panes()[1]?.dataset.active).toBe("true");

    await menuAction("Split down");
    expect(panes()).toHaveLength(3);
    expect(panes()[2]?.dataset.active).toBe("true");

    await click(getButton('[aria-label="New tab"]'));
    expect(panes()).toHaveLength(4);
    await click(tabs()[0]!);
    expect(panes()[2]?.dataset.active).toBe("true");
  });

  it("requires explicit confirmation before closing a live pane", async () => {
    await render();
    await menuAction("Split right");
    const paneClose = panes()[0]!.querySelector<HTMLButtonElement>("button")!;
    await click(paneClose);
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(panes()).toHaveLength(2);

    await click(button("Cancel"));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(panes()).toHaveLength(2);

    await click(paneClose);
    await click(button("Close pane"));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(panes()).toHaveLength(1);
  });

  it("confirms tab close and can return to an empty workspace", async () => {
    await render();
    await click(getButton('[aria-label="New tab"]'));
    const firstTabClose = document.querySelector<HTMLButtonElement>(
      ".terminal-tab-close",
    )!;
    await click(firstTabClose);
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await click(button("Cancel"));
    expect(tabs()).toHaveLength(2);

    await click(firstTabClose);
    await click(button("Close tab"));
    expect(tabs()).toHaveLength(1);
    expect(panes()).toHaveLength(1);

    await click(
      document.querySelector<HTMLButtonElement>(".terminal-tab-close")!,
    );
    await click(button("Close tab"));
    expect(tabs()).toHaveLength(0);
    expect(document.querySelector(".terminal-empty")).not.toBeNull();

    await click(button("New terminal"));
    expect(tabs()).toHaveLength(1);
    expect(panes()).toHaveLength(1);
  });

  it("routes workspace shortcuts without taking terminal control keys", async () => {
    await render();
    const pane = panes()[0]!;
    await act(async () => {
      expect(press(pane, { key: "t", ctrlKey: true, shiftKey: true })).toBe(
        false,
      );
    });
    expect(tabs()).toHaveLength(2);

    await act(async () => {
      press(panes()[1]!, { key: "d", ctrlKey: true, shiftKey: true });
    });
    expect(panes()).toHaveLength(3);

    const activeBefore = panes().findIndex(
      (candidate) => candidate.dataset.active === "true",
    );
    await act(async () => {
      press(panes()[activeBefore]!, {
        key: "ArrowRight",
        ctrlKey: true,
        shiftKey: true,
      });
    });
    const activeAfter = panes().findIndex(
      (candidate) => candidate.dataset.active === "true",
    );
    expect(activeAfter).not.toBe(activeBefore);

    await act(async () => {
      expect(press(panes()[activeAfter]!, { key: "c", ctrlKey: true })).toBe(
        true,
      );
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();

    await act(async () => {
      press(panes()[activeAfter]!, { key: "w", ctrlKey: true, shiftKey: true });
    });
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await act(async () => {
      press(document.querySelector('[role="dialog"]')!, { key: "Escape" });
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("ignores workspace shortcuts while an overlay is active", async () => {
    await render(false);
    await act(async () => {
      press(panes()[0]!, { key: "t", ctrlKey: true, shiftKey: true });
    });
    expect(tabs()).toHaveLength(1);
  });

  it("supports keyboard tab selection", async () => {
    await render();
    await click(getButton('[aria-label="New tab"]'));
    await act(async () => {
      press(tabs()[1]!, { key: "ArrowLeft" });
    });
    expect(tabs()[0]?.getAttribute("aria-selected")).toBe("true");
    await act(async () => {
      press(tabs()[0]!, { key: "End" });
    });
    expect(tabs()[1]?.getAttribute("aria-selected")).toBe("true");
  });

  it("maximizes and restores the active pane without unmounting siblings", async () => {
    await render();
    await menuAction("Split right");
    const splitPanes = panes();
    await menuAction("Maximize pane");
    expect(splitPanes[1]?.dataset.zoomed).toBe("true");
    expect(splitPanes[0]?.dataset.zoomHidden).toBe("true");
    expect(panes()).toHaveLength(2);
    await menuAction("Restore pane");
    expect(splitPanes[0]?.dataset.zoomHidden).toBe("false");
    expect(splitPanes[1]?.dataset.zoomHidden).toBe("false");
  });

  it("resizes a split with keyboard on its separator", async () => {
    await render();
    await menuAction("Split right");
    const divider = document.querySelector<HTMLElement>(
      '[data-testid="terminal-split-divider"]',
    );
    expect(divider).not.toBeNull();
    expect(divider?.getAttribute("aria-valuenow")).toBe("50");
    await act(async () => {
      divider?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });
    expect(divider?.getAttribute("aria-valuenow")).toBe("55");
  });

  it("renames and reorders tabs", async () => {
    const ref = createRef<TerminalWorkspaceHandle>();
    await render(true, ref);
    const firstTab = tabs()[0]!;
    dblclick(firstTab);
    const input = document.querySelector<HTMLInputElement>(
      ".terminal-tab-rename",
    );
    expect(input).not.toBeNull();
    await setInput(input!, "Build shell");
    await act(async () => {
      input?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(tabs()[0]?.textContent).toContain("Build shell");

    await click(getButton('[aria-label="New tab"]'));
    const secondTitle = tabs()[1]?.textContent;
    await act(async () => {
      ref.current?.moveActiveTab(-1);
    });
    expect(tabs()[0]?.textContent).toContain(secondTitle ?? "");
    expect(tabs()[1]?.textContent).toContain("Build shell");
  });

  it("derives pane titles from the renamed tab and unsplits explicitly", async () => {
    await render();
    dblclick(tabs()[0]!);
    const rename = document.querySelector<HTMLInputElement>(
      ".terminal-tab-rename",
    );
    await setInput(rename!, "Build shell");
    await act(async () => {
      rename?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    await menuAction("Split right");
    expect(panes().map((pane) => pane.dataset.title)).toEqual([
      "Build shell · 1",
      "Build shell · 2",
    ]);

    await menuAction("Unsplit tab");
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(panes()).toHaveLength(2);
    await click(button("Unsplit tab"));
    expect(panes()).toHaveLength(1);
    expect(panes()[0]?.dataset.title).toBe("Build shell");
  });

  it("closes other tabs only after confirmation", async () => {
    await render();
    await click(getButton('[aria-label="New tab"]'));
    await menuAction("Close other tabs");
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(tabs()).toHaveLength(2);
    await click(button("Cancel"));
    expect(tabs()).toHaveLength(2);
    await menuAction("Close other tabs");
    await click(button("Close other tabs"));
    expect(tabs()).toHaveLength(1);
    expect(panes()).toHaveLength(1);
  });

  it("keeps four panes available with one active pane", async () => {
    await render();
    await menuAction("Split right");
    await menuAction("Split down");
    await menuAction("Split right");
    expect(panes()).toHaveLength(4);
    expect(
      panes().filter((pane) => pane.dataset.active === "true"),
    ).toHaveLength(1);
  });
});
