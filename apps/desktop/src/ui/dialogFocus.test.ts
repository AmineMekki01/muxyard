// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

import { trapTabKey } from "./dialogFocus";

afterEach(() => {
  document.body.replaceChildren();
});

function event(shiftKey: boolean) {
  return {
    key: "Tab",
    shiftKey,
    preventDefault: vi.fn(),
  };
}

describe("dialog focus trap", () => {
  it("cycles forward from the last control and backward from the first", () => {
    const container = document.createElement("div");
    const first = document.createElement("button");
    const second = document.createElement("button");
    container.append(first, second);
    document.body.append(container);

    second.focus();
    const forward = event(false);
    expect(trapTabKey(forward, container)).toBe(true);
    expect(forward.preventDefault).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(first);

    const backward = event(true);
    expect(trapTabKey(backward, container)).toBe(true);
    expect(document.activeElement).toBe(second);
  });

  it("ignores non-Tab keys and empty containers", () => {
    const container = document.createElement("div");
    expect(
      trapTabKey(
        { key: "Escape", shiftKey: false, preventDefault: vi.fn() },
        container,
      ),
    ).toBe(false);
    expect(trapTabKey(event(false), container)).toBe(false);
  });
});
