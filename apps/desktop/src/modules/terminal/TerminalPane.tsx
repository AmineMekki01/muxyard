/** Imperative xterm.js surface; PTY bytes never pass through React state. */

import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useCallback, useEffect, useRef, useState } from "react";

import type { TerminalSpawnResponse } from "@muxyard/contracts";

import {
  attachTerminal,
  closeTerminal,
  inputTerminal,
  resizeTerminal,
  spawnTerminal,
} from "./terminal";

export function TerminalPane() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [session, setSession] = useState<TerminalSpawnResponse | null>(null);
  const [searchText, setSearchText] = useState("");

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let active = true;
    let streamId: string | null = null;
    let terminal: Terminal | null = null;
    let webgl: WebglAddon | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let dataDisposable: { dispose: () => void } | null = null;
    let resizeDisposable: { dispose: () => void } | null = null;
    let animationFrame = 0;

    setError(null);
    setSession(null);

    const run = async () => {
      const info = await spawnTerminal({
        cwd: null,
        shell: null,
        rows: 24,
        cols: 80,
      });
      if (!active) {
        await closeTerminal(info.stream_id).catch(() => {});
        return;
      }
      streamId = info.stream_id;
      setSession(info);

      terminal = new Terminal({
        allowProposedApi: true,
        cursorBlink: true,
        fontFamily:
          '"SFMono-Regular", "Cascadia Code", "Liberation Mono", monospace',
        fontSize: 13,
        lineHeight: 1.2,
        scrollback: 5000,
        theme: {
          background: "#0f1a1f",
          foreground: "#d8e5e8",
          cursor: "#8fd3e3",
          selectionBackground: "#2f6879",
        },
      });
      terminalRef.current = terminal;
      const fitAddon = new FitAddon();
      const searchAddon = new SearchAddon();
      searchAddonRef.current = searchAddon;
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(searchAddon);
      terminal.open(container);
      terminal.focus();

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
          // The pane may be hidden while a tab is changing.
        }
      };
      fit();
      resizeObserver = new ResizeObserver(() => {
        cancelAnimationFrame(animationFrame);
        animationFrame = requestAnimationFrame(fit);
      });
      resizeObserver.observe(container);

      const channel = new Channel<ArrayBuffer>();
      channel.onmessage = (chunk) => {
        if (active) terminal?.write(new Uint8Array(chunk));
      };
      await attachTerminal(info.stream_id, channel);
      if (!active) return;

      dataDisposable = terminal.onData((data) => {
        if (active && streamId) {
          void inputTerminal(streamId, data).catch(() => {});
        }
      });
      resizeDisposable = terminal.onResize(({ rows, cols }) => {
        if (active && streamId) {
          void resizeTerminal(streamId, rows, cols).catch(() => {});
        }
      });
      fit();
    };

    void run().catch((reason: unknown) => {
      if (active) {
        setError(
          reason instanceof Error
            ? reason.message
            : "The local terminal could not be started.",
        );
      }
    });

    return () => {
      active = false;
      cancelAnimationFrame(animationFrame);
      resizeObserver?.disconnect();
      dataDisposable?.dispose();
      resizeDisposable?.dispose();
      webgl?.dispose();
      searchAddonRef.current = null;
      terminalRef.current = null;
      terminal?.dispose();
      if (streamId) {
        void closeTerminal(streamId).catch(() => {});
      }
    };
  }, [attempt]);

  const findNext = useCallback(() => {
    if (searchText && searchAddonRef.current) {
      searchAddonRef.current.findNext(searchText);
    }
  }, [searchText]);

  const copySelection = useCallback(() => {
    const selection = terminalRef.current?.getSelection();
    if (selection) {
      void navigator.clipboard?.writeText(selection).catch(() => {});
      terminalRef.current?.focus();
    }
  }, []);

  return (
    <section className="terminal-panel" aria-label="Terminal">
      <header className="terminal-toolbar">
        <div className="terminal-identity">
          <strong>Local shell</strong>
          <span>{session?.cwd ?? "Starting terminal"}</span>
        </div>
        <div className="terminal-actions">
          <input
            className="terminal-search"
            type="search"
            aria-label="Search terminal"
            placeholder="Search"
            value={searchText}
            onChange={(event) => setSearchText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                findNext();
              }
            }}
          />
          <button type="button" className="shell-button" onClick={findNext}>
            Find next
          </button>
          <button
            type="button"
            className="shell-button"
            onClick={copySelection}
          >
            Copy selection
          </button>
        </div>
      </header>
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
