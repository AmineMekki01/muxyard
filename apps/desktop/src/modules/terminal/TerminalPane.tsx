/** Imperative xterm.js surface; PTY bytes never pass through React state. */

import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useCallback, useEffect, useRef, useState } from "react";

import type { TerminalSpawnResponse } from "@muxyard/contracts";

import { createLinkProvider } from "./links";
import { TerminalIcon } from "./TerminalIcon";
import { createResizeScheduler } from "./resize";
import {
  attachTerminal,
  closeTerminal,
  getTerminalRenderConfig,
  inputTerminal,
  resizeTerminal,
  snapshotTerminal,
  spawnTerminal,
} from "./terminal";

export const DEFAULT_SCROLLBACK_LINES = 10_000;
export const HIDDEN_OUTPUT_LIMIT_BYTES = 256 * 1024;

const HIDDEN_TRUNCATION_MARKER = "\r\n[Earlier hidden output truncated]\r\n";

const SEARCH_OPTIONS = {
  decorations: {
    matchBackground: "#3b4f58",
    matchBorder: "#56d4dd",
    matchOverviewRuler: "#56d4dd",
    activeMatchBackground: "#7a5c20",
    activeMatchBorder: "#e3b341",
    activeMatchColorOverviewRuler: "#e3b341",
  },
};

type TerminalPaneProps = {
  title?: string;
  active?: boolean;
  zoomed?: boolean;
  zoomHidden?: boolean;
  sessionId?: string | null;
  sessionLost?: boolean;
  historyId?: string;
  cwd?: string | null;
  shell?: string | null;
  scrollOffset?: number;
  onViewport?: (offset: number) => void;
  onFocus?: () => void;
  onClose?: () => void;
  onToggleZoom?: () => void;
  onSession?: (info: TerminalSpawnResponse) => void;
  onLost?: () => void;
};

export function TerminalPane({
  title = "Local shell",
  active = true,
  zoomed = false,
  zoomHidden = false,
  sessionId = null,
  sessionLost = false,
  historyId,
  cwd,
  shell,
  scrollOffset = 0,
  onViewport,
  onFocus,
  onClose,
  onToggleZoom,
  onSession,
  onLost,
}: TerminalPaneProps = {}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;
  const zoomHiddenRef = useRef(zoomHidden);
  zoomHiddenRef.current = zoomHidden;
  const hiddenOutputRef = useRef({
    chunks: [] as Uint8Array[],
    bytes: 0,
    truncated: false,
  });
  const [attempt, setAttempt] = useState(0);
  const [forceNew, setForceNew] = useState(false);
  const [restoreFailed, setRestoreFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [session, setSession] = useState<TerminalSpawnResponse | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchText, setSearchText] = useState("");
  const [searchResults, setSearchResults] = useState({
    index: -1,
    count: 0,
  });
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const isLost = (sessionLost || restoreFailed) && !forceNew;

  useEffect(() => {
    if (isLost) return;
    const container = containerRef.current;
    if (!container) return;

    let alive = true;
    let streamId: string | null = null;
    let terminal: Terminal | null = null;
    let webgl: WebglAddon | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let resizeScheduler: ReturnType<typeof createResizeScheduler> | null = null;
    let dataDisposable: { dispose: () => void } | null = null;
    let resizeDisposable: { dispose: () => void } | null = null;
    let scrollDisposable: { dispose: () => void } | null = null;
    let searchResultsDisposable: { dispose: () => void } | null = null;
    let linkProviderDisposable: { dispose: () => void } | null = null;

    setError(null);
    setSession(null);
    hiddenOutputRef.current = { chunks: [], bytes: 0, truncated: false };

    const enqueueHiddenOutput = (chunk: Uint8Array) => {
      const buffer = hiddenOutputRef.current;
      buffer.chunks.push(chunk.slice());
      buffer.bytes += chunk.byteLength;
      while (
        buffer.bytes > HIDDEN_OUTPUT_LIMIT_BYTES &&
        buffer.chunks.length > 0
      ) {
        const excess = buffer.bytes - HIDDEN_OUTPUT_LIMIT_BYTES;
        const first = buffer.chunks[0]!;
        if (first.byteLength <= excess) {
          buffer.chunks.shift();
          buffer.bytes -= first.byteLength;
        } else {
          buffer.chunks[0] = first.slice(excess);
          buffer.bytes -= excess;
        }
        buffer.truncated = true;
      }
    };

    const run = async () => {
      const renderConfig = await getTerminalRenderConfig().catch(() => null);
      const scrollback =
        renderConfig?.scrollback_lines ?? DEFAULT_SCROLLBACK_LINES;
      const restoring = Boolean(sessionId) && !forceNew;
      const info = restoring
        ? {
            stream_id: sessionId!,
            shell: shell ?? "Shell",
            cwd: cwd ?? "",
            rows: 24,
            cols: 80,
          }
        : await spawnTerminal({
            cwd: cwd ?? null,
            shell: shell ?? null,
            history_id: historyId ?? null,
            rows: 24,
            cols: 80,
          });
      if (!alive) {
        if (!restoring) await closeTerminal(info.stream_id).catch(() => {});
        return;
      }
      streamId = info.stream_id;
      setSession(info);
      if (!restoring) onSession?.(info);

      terminal = new Terminal({
        allowProposedApi: true,
        cursorBlink: true,
        cursorStyle: "bar",
        cursorWidth: 2,
        cursorInactiveStyle: "outline",
        drawBoldTextInBrightColors: true,
        fontFamily:
          '"SF Mono", ".SF NS Mono", Menlo, Monaco, "Cascadia Code", monospace',
        fontSize: 14,
        fontWeight: "400",
        fontWeightBold: "600",
        letterSpacing: 0.1,
        lineHeight: 1.35,
        minimumContrastRatio: 4.5,
        scrollback,
        theme: {
          background: "#0f1a1f",
          foreground: "#e6edf0",
          cursor: "#7dd3fc",
          cursorAccent: "#0f1a1f",
          selectionBackground: "#264f5c",
          selectionInactiveBackground: "#1d3a44",
          selectionForeground: "#f0f6fc",
          black: "#16242b",
          red: "#ff7b72",
          green: "#7ee787",
          yellow: "#e3b341",
          blue: "#79c0ff",
          magenta: "#d2a8ff",
          cyan: "#56d4dd",
          white: "#d0d7de",
          brightBlack: "#6e7f8a",
          brightRed: "#ffa198",
          brightGreen: "#aff5b4",
          brightYellow: "#f8e3a1",
          brightBlue: "#a5d6ff",
          brightMagenta: "#e2c5ff",
          brightCyan: "#9be9ef",
          brightWhite: "#f0f6fc",
        },
      });
      terminalRef.current = terminal;
      const fitAddon = new FitAddon();
      const searchAddon = new SearchAddon();
      fitAddonRef.current = fitAddon;
      searchAddonRef.current = searchAddon;
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(searchAddon);
      searchResultsDisposable = searchAddon.onDidChangeResults(
        ({ resultIndex, resultCount }) => {
          setSearchResults({ index: resultIndex, count: resultCount });
        },
      );
      terminal.open(container);
      linkProviderDisposable = terminal.registerLinkProvider(
        createLinkProvider(terminal),
      );
      if (activeRef.current) terminal.focus();

      try {
        webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          webgl?.dispose();
          webgl = null;
        });
        terminal.loadAddon(webgl);
      } catch {
        webgl?.dispose();
        webgl = null;
      }

      const fit = () => {
        try {
          fitAddon.fit();
        } catch {
          // The pane may be hidden while a tab or split changes.
        }
      };
      fit();
      resizeScheduler = createResizeScheduler(fit);
      resizeObserver = new ResizeObserver(() => {
        resizeScheduler?.schedule();
      });
      resizeObserver.observe(container);

      if (restoring) {
        const snapshot = await snapshotTerminal(info.stream_id).catch(
          () => null,
        );
        if (alive && snapshot?.data?.length) {
          terminal.write(new Uint8Array(snapshot.data));
        }
        if (alive && scrollOffset > 0) {
          const buffer = terminal.buffer.active;
          terminal.scrollToLine(Math.max(0, buffer.baseY - scrollOffset));
        }
      }

      scrollDisposable = terminal.onScroll(() => {
        const buffer = terminal?.buffer.active;
        if (buffer) onViewport?.(Math.max(0, buffer.baseY - buffer.viewportY));
      });

      const channel = new Channel<ArrayBuffer>();
      channel.onmessage = (chunk) => {
        if (!alive) return;
        const bytes = new Uint8Array(chunk);
        if (activeRef.current && !zoomHiddenRef.current) {
          terminal?.write(bytes);
        } else {
          enqueueHiddenOutput(bytes);
        }
      };
      try {
        await attachTerminal(info.stream_id, channel);
      } catch (reason) {
        if (restoring) {
          setRestoreFailed(true);
          onLost?.();
          return;
        }
        throw reason;
      }
      if (!alive) return;

      dataDisposable = terminal.onData((data) => {
        if (alive && streamId) {
          void inputTerminal(streamId, data).catch(() => {});
        }
      });
      resizeDisposable = terminal.onResize(({ rows, cols }) => {
        if (alive && streamId) {
          void resizeTerminal(streamId, rows, cols).catch(() => {});
        }
      });
      fit();
      // Force a SIGWINCH after attach so a prompt emitted before the
      // subscription cannot leave the pane visually blank.
      void resizeTerminal(info.stream_id, terminal.rows, terminal.cols).catch(
        () => {},
      );
    };

    void run().catch((reason: unknown) => {
      if (alive) {
        setError(
          reason instanceof Error
            ? reason.message
            : "The local terminal could not be started.",
        );
      }
    });

    return () => {
      alive = false;
      resizeScheduler?.cancel();
      resizeObserver?.disconnect();
      dataDisposable?.dispose();
      resizeDisposable?.dispose();
      scrollDisposable?.dispose();
      searchResultsDisposable?.dispose();
      linkProviderDisposable?.dispose();
      webgl?.dispose();
      fitAddonRef.current = null;
      searchAddonRef.current = null;
      terminalRef.current = null;
      terminal?.dispose();
      // Detach only. Host sessions survive desktop UI exit for reattachment.
    };
  }, [attempt, forceNew, isLost]);

  useEffect(() => {
    if (!active || zoomHidden) return;
    const terminal = terminalRef.current;
    const buffer = hiddenOutputRef.current;
    if (terminal) {
      for (const chunk of buffer.chunks) terminal.write(chunk);
      // Keep the marker visible after the retained tail; otherwise xterm's
      // bounded scrollback can push a leading marker out of the viewport.
      if (buffer.truncated) terminal.write(HIDDEN_TRUNCATION_MARKER);
    }
    hiddenOutputRef.current = { chunks: [], bytes: 0, truncated: false };
    try {
      fitAddonRef.current?.fit();
    } catch {
      // The pane may not have finished mounting yet.
    }
    terminal?.focus();
  }, [active, zoomed, zoomHidden]);

  useEffect(() => {
    if (!searchOpen) return;
    searchInputRef.current?.focus();
  }, [searchOpen]);

  const findNext = useCallback(() => {
    if (searchText && searchAddonRef.current) {
      searchAddonRef.current.findNext(searchText, SEARCH_OPTIONS);
    }
  }, [searchText]);

  const findPrevious = useCallback(() => {
    if (searchText && searchAddonRef.current) {
      searchAddonRef.current.findPrevious(searchText, SEARCH_OPTIONS);
    }
  }, [searchText]);

  const closeSearch = () => {
    searchAddonRef.current?.clearDecorations();
    setSearchResults({ index: -1, count: 0 });
    setSearchOpen(false);
    terminalRef.current?.focus();
  };

  const copySelection = useCallback(() => {
    const selection = terminalRef.current?.getSelection();
    if (selection) {
      void navigator.clipboard?.writeText(selection).catch(() => {});
      terminalRef.current?.focus();
    }
  }, []);

  const startFresh = () => {
    if (sessionId) void closeTerminal(sessionId).catch(() => {});
    setRestoreFailed(false);
    setForceNew(true);
    setAttempt((current) => current + 1);
  };

  if (isLost) {
    return (
      <section
        className="terminal-panel terminal-panel-lost"
        aria-label={title}
        data-terminal-focus="true"
        data-active={active ? "true" : "false"}
        data-zoomed={zoomed ? "true" : "false"}
        data-zoom-hidden={zoomHidden ? "true" : "false"}
        aria-hidden={zoomHidden ? true : undefined}
        onFocusCapture={() => onFocus?.()}
        onMouseDown={() => onFocus?.()}
      >
        <header className="terminal-toolbar">
          <div className="terminal-identity">
            <strong>{title}</strong>
            <span>{cwd ?? "Session unavailable"}</span>
          </div>
          <div className="terminal-actions">
            {onToggleZoom ? (
              <button
                type="button"
                className="terminal-icon-button"
                aria-label={`Restore split layout for ${title}`}
                title="Restore split layout"
                aria-pressed={zoomed}
                onClick={onToggleZoom}
              >
                <TerminalIcon name={zoomed ? "restore" : "maximize"} />
              </button>
            ) : null}
            {onClose ? (
              <button
                type="button"
                className="terminal-icon-button terminal-close-button"
                aria-label={`Close pane ${title}`}
                title={`Close pane ${title}`}
                onClick={onClose}
              >
                <TerminalIcon name="close" />
              </button>
            ) : null}
          </div>
        </header>
        <div className="terminal-lost" role="status">
          <p>This terminal session is no longer running.</p>
          <button type="button" className="primary-button" onClick={startFresh}>
            Start new shell
          </button>
        </div>
      </section>
    );
  }

  return (
    <section
      className="terminal-panel"
      aria-label={title}
      data-terminal-focus="true"
      data-active={active ? "true" : "false"}
      data-zoomed={zoomed ? "true" : "false"}
      data-zoom-hidden={zoomHidden ? "true" : "false"}
      aria-hidden={zoomHidden ? true : undefined}
      onFocusCapture={() => onFocus?.()}
      onMouseDown={() => onFocus?.()}
      onKeyDownCapture={(event) => {
        if (
          event.metaKey &&
          !event.ctrlKey &&
          !event.altKey &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          setSearchOpen(true);
        }
      }}
    >
      <header className="terminal-toolbar">
        <div className="terminal-identity">
          <strong>{title}</strong>
          <span>{session?.cwd ?? cwd ?? "Starting terminal"}</span>
        </div>
        <div className="terminal-actions">
          <button
            type="button"
            className="terminal-icon-button"
            aria-label={`Search ${title}`}
            title={`Search ${title}`}
            aria-expanded={searchOpen}
            onClick={() => setSearchOpen((current) => !current)}
          >
            <TerminalIcon name="search" />
          </button>
          <button
            type="button"
            className="terminal-icon-button"
            aria-label={`Copy selection from ${title}`}
            title={`Copy selection from ${title}`}
            onClick={copySelection}
          >
            <TerminalIcon name="copy" />
          </button>
          {onToggleZoom ? (
            <button
              type="button"
              className="terminal-icon-button"
              aria-label={
                zoomed
                  ? `Restore split layout for ${title}`
                  : `Maximize ${title}`
              }
              title={zoomed ? "Restore split layout" : "Maximize pane"}
              aria-pressed={zoomed}
              onClick={onToggleZoom}
            >
              <TerminalIcon name={zoomed ? "restore" : "maximize"} />
            </button>
          ) : null}
          {onClose ? (
            <button
              type="button"
              className="terminal-icon-button terminal-close-button"
              aria-label={`Close pane ${title}`}
              title={`Close pane ${title}`}
              onClick={onClose}
            >
              <TerminalIcon name="close" />
            </button>
          ) : null}
        </div>
      </header>
      {searchOpen ? (
        <div className="terminal-searchbar">
          <TerminalIcon name="search" />
          <input
            ref={searchInputRef}
            className="terminal-search"
            type="search"
            aria-label={`Search text in ${title}`}
            placeholder="Search terminal"
            value={searchText}
            onChange={(event) => setSearchText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                if (event.shiftKey) {
                  findPrevious();
                } else {
                  findNext();
                }
              } else if (event.key === "Escape") {
                event.preventDefault();
                closeSearch();
              }
            }}
          />
          <span className="terminal-search-count" aria-live="polite">
            {searchResults.count > 0
              ? `${searchResults.index + 1}/${searchResults.count}`
              : "0/0"}
          </span>
          <button
            type="button"
            className="shell-button"
            aria-label="Previous search result"
            onClick={findPrevious}
          >
            Prev
          </button>
          <button type="button" className="shell-button" onClick={findNext}>
            Next
          </button>
          <button
            type="button"
            className="terminal-icon-button"
            aria-label="Close terminal search"
            onClick={closeSearch}
          >
            <TerminalIcon name="close" />
          </button>
        </div>
      ) : null}
      <div className="terminal-surface" ref={containerRef} />
      {error ? (
        <div className="terminal-error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="secondary-button"
            onClick={() => setAttempt((current) => current + 1)}
          >
            Retry terminal
          </button>
        </div>
      ) : null}
    </section>
  );
}
