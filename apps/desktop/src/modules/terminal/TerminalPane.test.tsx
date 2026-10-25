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
    emitData: (data: string) => void;
    emitResize: (rows: number, cols: number) => void;
  },
  failWebgl: false,
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
    write = vi.fn();
    dispose = vi.fn();
    focus = vi.fn();
    open = vi.fn();
    getSelection = vi.fn(() => "selected text");
    private dataHandler: ((data: string) => void) | null = null;
    private resizeHandler:
      ((size: { rows: number; cols: number }) => void) | null = null;

    constructor() {
      state.terminal = this as unknown as NonNullable<typeof state.terminal>;
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
  },
}));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn();
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
  state.failWebgl = false;
  state.webgl = null;
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

async function mount() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<TerminalPane />);
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
    expect(state.terminal?.focus).toHaveBeenCalled();

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

  it("disposes the renderer and closes the host session on unmount", async () => {
    await mount();
    const terminal = state.terminal;
    await act(async () => {
      root?.unmount();
    });
    root = null;
    expect(terminal?.dispose).toHaveBeenCalled();
    expect(state.invoke).toHaveBeenCalledWith(
      "terminal_close",
      expect.objectContaining({
        request: { stream_id: "12345678-aaaa-bbbb-cccc-1234567890ab" },
      }),
    );
  });
});
