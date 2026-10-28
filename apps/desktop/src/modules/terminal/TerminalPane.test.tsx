// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

(
  globalThis as typeof globalThis & {
    IS_REACT_ACT_ENVIRONMENT: boolean;
  }
).IS_REACT_ACT_ENVIRONMENT = true;

const state = vi.hoisted(() => ({
  invoke: vi.fn(),
  channel: null as null | { onmessage: (value: ArrayBuffer) => void },
  terminal: null as null | {
    write: ReturnType<typeof vi.fn>;
    dispose: ReturnType<typeof vi.fn>;
    focus: ReturnType<typeof vi.fn>;
    open: ReturnType<typeof vi.fn>;
    scrollToLine: ReturnType<typeof vi.fn>;
    buffer: { active: { baseY: number; viewportY: number } };
    registerLinkProvider: ReturnType<typeof vi.fn>;
    emitData: (data: string) => void;
    emitResize: (rows: number, cols: number) => void;
    emitScroll: () => void;
  },
  terminalOptions: null as null | { scrollback?: number },
  failWebgl: false,
  fit: null as null | { fit: ReturnType<typeof vi.fn> },
  search: null as null | {
    findNext: ReturnType<typeof vi.fn>;
    findPrevious: ReturnType<typeof vi.fn>;
    clearDecorations: ReturnType<typeof vi.fn>;
    onDidChangeResults: ReturnType<typeof vi.fn>;
  },
  webgl: null as null | {
    dispose: ReturnType<typeof vi.fn>;
    triggerContextLoss: () => void;
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (value: ArrayBuffer) => void = () => {};

    constructor() {
      state.channel = this as unknown as {
        onmessage: (value: ArrayBuffer) => void;
      };
    }
  },
  invoke: state.invoke,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    write = vi.fn();
    dispose = vi.fn();
    focus = vi.fn();
    open = vi.fn();
    getSelection = vi.fn(() => "selected text");
    buffer = { active: { baseY: 10, viewportY: 10 } };
    scrollToLine = vi.fn();
    private dataHandler: ((data: string) => void) | null = null;
    private scrollHandler: (() => void) | null = null;
    private resizeHandler:
      ((size: { rows: number; cols: number }) => void) | null = null;

    constructor(options?: { scrollback?: number }) {
      state.terminal = this as unknown as NonNullable<typeof state.terminal>;
      state.terminalOptions = options ?? null;
    }

    registerLinkProvider() {
      return { dispose: vi.fn() };
    }

    loadAddon() {}
    onData(handler: (data: string) => void) {
      this.dataHandler = handler;
      return { dispose: vi.fn() };
    }
    onResize(handler: (size: { rows: number; cols: number }) => void) {
      this.resizeHandler = handler;
      return { dispose: vi.fn() };
    }
    onScroll(handler: () => void) {
      this.scrollHandler = handler;
      return { dispose: vi.fn() };
    }
    emitScroll() {
      this.scrollHandler?.();
    }
    emitData(data: string) {
      this.dataHandler?.(data);
    }
    emitResize(rows: number, cols: number) {
      this.resizeHandler?.({ rows, cols });
    }
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();

    constructor() {
      state.fit = this as unknown as NonNullable<typeof state.fit>;
    }
  },
}));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn();
    findPrevious = vi.fn();
    clearDecorations = vi.fn();
    onDidChangeResults = vi.fn(() => ({ dispose: vi.fn() }));

    constructor() {
      state.search = this as unknown as NonNullable<typeof state.search>;
    }
  },
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    dispose = vi.fn();
    private contextLoss: (() => void) | null = null;

    constructor() {
      if (state.failWebgl) throw new Error("WebGL unavailable");
      state.webgl = this as unknown as NonNullable<typeof state.webgl>;
    }

    onContextLoss(handler: () => void) {
      this.contextLoss = handler;
    }

    triggerContextLoss() {
      this.contextLoss?.();
    }
  },
}));

import { TerminalPane } from "./TerminalPane";

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", MockResizeObserver);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  state.invoke.mockReset();
  state.channel = null;
  state.terminal = null;
  state.terminalOptions = null;
  state.failWebgl = false;
  state.fit = null;
  state.search = null;
  state.webgl = null;
  state.invoke.mockImplementation((command: string, args: unknown) => {
    if (command === "terminal_render_config") {
      return Promise.resolve({ scrollback_lines: 12000 });
    }
    if (command === "terminal_spawn") {
      return Promise.resolve({
        stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
        shell: "/bin/sh",
        cwd: "/tmp",
        rows: 24,
        cols: 80,
      });
    }
    if (command === "terminal_attach") {
      state.channel = (args as { channel: typeof state.channel }).channel;
      return Promise.resolve();
    }
    return Promise.resolve();
  });
});

afterEach(() => {
  if (root) {
    act(() => root?.unmount());
    root = null;
  }
  host?.remove();
  host = null;
  vi.unstubAllGlobals();
});

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

async function mount(active = true) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<TerminalPane active={active} />);
  });
  await vi.waitFor(() => {
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_attach",
      expect.objectContaining({
        stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
      }),
    );
  });
}

describe("terminal pane", () => {
  it("spawns, attaches, writes output directly, and forwards input", async () => {
    await mount();
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_spawn",
      expect.objectContaining({
        request: expect.objectContaining({ rows: 24, cols: 80 }),
      }),
    );
    expect(state.terminal).not.toBeNull();
    expect(state.terminalOptions?.scrollback).toBe(12000);
    expect(state.terminal?.focus).toHaveBeenCalled();
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_resize",
      expect.objectContaining({
        request: expect.objectContaining({ rows: 24, cols: 80 }),
      }),
    );

    const bytes = new Uint8Array([104, 105]);
    act(() => {
      state.channel?.onmessage(bytes.buffer);
    });
    expect(state.terminal?.write).toHaveBeenCalledWith(bytes);

    act(() => {
      state.terminal?.emitData("ls\n");
    });
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_input",
      expect.objectContaining({
        request: expect.objectContaining({
          stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
          data: "ls\n",
        }),
      }),
    );

    act(() => {
      state.terminal?.emitResize(30, 100);
    });
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_resize",
      expect.objectContaining({
        request: expect.objectContaining({ rows: 30, cols: 100 }),
      }),
    );
  });

  it("writes the bounded output tail before reattaching a saved session", async () => {
    state.invoke.mockImplementation((command: string, args: unknown) => {
      if (command === "terminal_snapshot") {
        return Promise.resolve({
          data: [114, 101, 115, 116, 111, 114, 101, 100],
        });
      }
      if (command === "terminal_attach") {
        state.channel = (args as { channel: typeof state.channel }).channel;
        return Promise.resolve();
      }
      return Promise.resolve();
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <TerminalPane
          sessionId="12345678-aaaa-bbbb-cccc-1234567890ab"
          historyId="pane-1"
          cwd="/tmp"
          shell="/bin/sh"
          scrollOffset={5}
        />,
      );
    });
    await vi.waitFor(() => {
      expect(state.invoke).toHaveBeenCalledWith(
        "terminal_attach",
        expect.objectContaining({
          stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
        }),
      );
    });
    expect(state.invoke).not.toHaveBeenCalledWith(
      "terminal_spawn",
      expect.anything(),
    );
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_snapshot",
      expect.objectContaining({
        request: { stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab" },
      }),
    );
    expect(state.terminal?.write).toHaveBeenCalledWith(
      new Uint8Array([114, 101, 115, 116, 111, 114, 101, 100]),
    );
    expect(state.terminal?.scrollToLine).toHaveBeenCalledWith(5);
  });

  it("reports the scroll offset from the bottom for persistence", async () => {
    const onViewport = vi.fn();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<TerminalPane onViewport={onViewport} />);
    });
    await vi.waitFor(() => {
      expect(state.invoke).toHaveBeenCalledWith(
        "terminal_attach",
        expect.anything(),
      );
    });
    if (state.terminal) {
      state.terminal.buffer.active = { baseY: 10, viewportY: 7 };
    }
    act(() => state.terminal?.emitScroll());
    expect(onViewport).toHaveBeenCalledWith(3);
  });

  it("copies the current terminal selection", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    await mount();
    const copyButton = document.querySelector<HTMLButtonElement>(
      '[aria-label="Copy selection from Local shell"]',
    );
    expect(copyButton).not.toBeNull();
    await act(async () => copyButton?.click());
    expect(writeText).toHaveBeenCalledWith("selected text");
  });

  it("keeps hidden output and focuses only when activated", async () => {
    await mount(false);
    expect(state.terminal?.focus).not.toHaveBeenCalled();
    const bytes = new Uint8Array([104, 105]);
    act(() => {
      state.channel?.onmessage(bytes.buffer);
    });
    expect(state.terminal?.write).toHaveBeenCalledWith(bytes);

    state.fit?.fit.mockClear();
    await act(async () => {
      root?.render(<TerminalPane active />);
    });
    expect(state.terminal?.focus).toHaveBeenCalled();
    expect(state.fit?.fit).toHaveBeenCalled();
  });

  it("opens the compact search bar and finds the next match", async () => {
    await mount();
    const searchButton = document.querySelector<HTMLButtonElement>(
      '[aria-label="Search Local shell"]',
    );
    expect(searchButton).not.toBeNull();
    await act(async () => searchButton?.click());
    const input = document.querySelector<HTMLInputElement>(
      '[aria-label="Search text in Local shell"]',
    );
    expect(input).not.toBeNull();
    await setInput(input!, "needle");
    await act(async () => {
      input?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(state.search?.findNext).toHaveBeenCalledWith(
      "needle",
      expect.objectContaining({
        decorations: expect.any(Object),
      }),
    );

    await act(async () => {
      input?.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          shiftKey: true,
          bubbles: true,
        }),
      );
    });
    expect(state.search?.findPrevious).toHaveBeenCalledWith(
      "needle",
      expect.objectContaining({
        decorations: expect.any(Object),
      }),
    );
  });

  it("shows a lost session and starts a fresh shell with the same history id", async () => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <TerminalPane
          title="Terminal 1"
          sessionId="12345678-aaaa-bbbb-cccc-1234567890ab"
          sessionLost
          historyId="pane-1"
          cwd="/tmp"
          shell="/bin/sh"
        />,
      );
    });
    expect(host.textContent).toContain("no longer running");
    expect(state.invoke).not.toHaveBeenCalledWith(
      "terminal_spawn",
      expect.anything(),
    );

    const start = Array.from(
      host.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent?.trim() === "Start new shell");
    expect(start).toBeDefined();
    await act(async () => start?.click());
    await vi.waitFor(() => {
      expect(state.invoke).toHaveBeenCalledWith(
        "terminal_spawn",
        expect.objectContaining({
          request: expect.objectContaining({ history_id: "pane-1" }),
        }),
      );
    });
  });

  it("marks an attach failure as lost without spawning a replacement", async () => {
    state.invoke.mockImplementation((command: string, args: unknown) => {
      if (command === "terminal_spawn") {
        return Promise.resolve({
          stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
          shell: "/bin/sh",
          cwd: "/tmp",
          rows: 24,
          cols: 80,
        });
      }
      if (command === "terminal_attach") {
        state.channel = (args as { channel: typeof state.channel }).channel;
        return Promise.reject(new Error("session missing"));
      }
      return Promise.resolve();
    });
    const onLost = vi.fn();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <TerminalPane
          title="Terminal 1"
          sessionId="12345678-aaaa-bbbb-cccc-1234567890ab"
          historyId="pane-1"
          cwd="/tmp"
          shell="/bin/sh"
          onLost={onLost}
        />,
      );
    });
    await vi.waitFor(() => {
      expect(onLost).toHaveBeenCalled();
    });
    expect(host.textContent).toContain("no longer running");
    expect(state.invoke).not.toHaveBeenCalledWith(
      "terminal_spawn",
      expect.anything(),
    );
  });

  it("falls back when WebGL is unavailable", async () => {
    state.failWebgl = true;
    await mount();
    expect(state.terminal?.open).toHaveBeenCalled();
    expect(host?.textContent).not.toContain("The local terminal could not");
  });

  it("disposes the WebGL renderer when its context is lost", async () => {
    await mount();
    act(() => {
      state.webgl?.triggerContextLoss();
    });
    expect(state.webgl?.dispose).toHaveBeenCalled();
    expect(state.terminal?.open).toHaveBeenCalled();
  });

  it("disposes the renderer without killing the host session on UI detach", async () => {
    await mount();
    const terminal = state.terminal;
    await act(async () => {
      root?.unmount();
    });
    root = null;
    expect(terminal?.dispose).toHaveBeenCalled();
    expect(state.invoke).not.toHaveBeenCalledWith(
      "terminal_close",
      expect.anything(),
    );
  });
});
