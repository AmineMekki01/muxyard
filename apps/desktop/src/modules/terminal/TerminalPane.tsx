/** Imperative xterm.js surface; PTY bytes never pass through React state. */

import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useCallback, useEffect, useRef, useState } from "react";

import type { TerminalSpawnResponse } from "@muxyard/contracts";

import { TerminalIcon } from "./TerminalIcon";
import { createResizeScheduler } from "./resize";
import {
  attachTerminal,
  closeTerminal,
  inputTerminal,
  resizeTerminal,
  spawnTerminal,
} from "./terminal";

type TerminalPaneProps = {
  title?: string;
  active?: boolean;
  zoomed?: boolean;
  zoomHidden?: boolean;
  onFocus?: () => void;
  onClose?: () => void;
  onToggleZoom?: () => void;
};

export function TerminalPane({
  title = "Local shell",
  active = true,
  zoomed = false,
  zoomHidden = false,
  onFocus,
  onClose,
  onToggleZoom,
}: TerminalPaneProps = {}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [session, setSession] = useState<TerminalSpawnResponse | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchText, setSearchText] = useState("");
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
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

    setError(null);
    setSession(null);

    const run = async () => {
      const info = await spawnTerminal({
        cwd: null,
        shell: null,
        rows: 24,
        cols: 80,
      });
      if (!alive) {
        await closeTerminal(info.stream_id).catch(() => {});
        return;
      }
      streamId = info.stream_id;
      setSession(info);

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
        scrollback: 5000,
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
      terminal.open(container);
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

      const channel = new Channel<ArrayBuffer>();
      channel.onmessage = (chunk) => {
        if (alive) terminal?.write(new Uint8Array(chunk));
      };
      await attachTerminal(info.stream_id, channel);
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
      webgl?.dispose();
      fitAddonRef.current = null;
      searchAddonRef.current = null;
      terminalRef.current = null;
      terminal?.dispose();
      if (streamId) {
        void closeTerminal(streamId).catch(() => {});
      }
    };
  }, [attempt]);

  useEffect(() => {
    if (!active || zoomHidden) return;
    try {
      fitAddonRef.current?.fit();
    } catch {
      // The pane may not have finished mounting yet.
    }
    terminalRef.current?.focus();
  }, [active, zoomed, zoomHidden]);

  useEffect(() => {
    if (!searchOpen) return;
    searchInputRef.current?.focus();
  }, [searchOpen]);

  const findNext = useCallback(() => {
    if (searchText && searchAddonRef.current) {
      searchAddonRef.current.findNext(searchText);
    }
  }, [searchText]);

  const closeSearch = () => {
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
          <span>{session?.cwd ?? "Starting terminal"}</span>
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
                findNext();
              } else if (event.key === "Escape") {
                event.preventDefault();
                closeSearch();
              }
            }}
          />
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
