// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";

import {
  isEditableTarget,
  isHelpShortcut,
  isPaletteShortcut,
  isTerminalTarget,
  nextEnabledIndex,
  shouldIgnoreGlobalShortcut,
} from "./keymap";

afterEach(() => {
  document.body.replaceChildren();
});

describe("keyboard routing", () => {
  it("recognizes the palette shortcut on macOS and Windows/Linux", () => {
    expect(
      isPaletteShortcut({
        key: "P",
        metaKey: true,
        ctrlKey: false,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(true);
    expect(
      isPaletteShortcut({
        key: "p",
        metaKey: false,
        ctrlKey: true,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(true);
    expect(
      isPaletteShortcut({
        key: "p",
        metaKey: false,
        ctrlKey: true,
        shiftKey: false,
        altKey: false,
      }),
    ).toBe(false);
  });

  it("recognizes the help shortcut only outside modifiers", () => {
    expect(
      isHelpShortcut({
        key: "?",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
      }),
    ).toBe(true);
    expect(
      isHelpShortcut({
        key: "?",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
      }),
    ).toBe(false);
  });

  it("does not treat terminal or editable focus as shell focus", () => {
    const input = document.createElement("input");
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");
    const terminal = document.createElement("div");
    terminal.dataset.terminalFocus = "true";
    const plain = document.createElement("div");
    document.body.append(input, editor, terminal, plain);

    expect(isEditableTarget(input)).toBe(true);
    expect(isEditableTarget(editor)).toBe(true);
    expect(isTerminalTarget(terminal)).toBe(true);
    expect(shouldIgnoreGlobalShortcut(terminal)).toBe(true);
    expect(shouldIgnoreGlobalShortcut(plain)).toBe(false);
  });

  it("wraps command selection and handles empty lists", () => {
    expect(nextEnabledIndex(0, 3, 1)).toBe(1);
    expect(nextEnabledIndex(2, 3, 1)).toBe(0);
    expect(nextEnabledIndex(0, 3, -1)).toBe(2);
    expect(nextEnabledIndex(0, 0, 1)).toBe(-1);
  });
});
