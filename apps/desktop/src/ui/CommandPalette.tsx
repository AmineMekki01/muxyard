/** Local layout and focus commands; no host or terminal actions are dispatched. */

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";

import {
  filterCommands,
  type AppCommand,
  type CommandContext,
} from "../app/commands";
import { nextEnabledIndex } from "../app/keymap";
import { trapTabKey, useDialogFocus } from "./dialogFocus";

type CommandPaletteProps = {
  open: boolean;
  commands: readonly AppCommand[];
  context: CommandContext;
  onClose: () => void;
};

export function CommandPalette({
  open,
  commands,
  context,
  onClose,
}: CommandPaletteProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const filtered = useMemo(
    () => filterCommands(commands, query),
    [commands, query],
  );

  useDialogFocus(open, inputRef);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
  }, [open]);

  useEffect(() => {
    setActiveIndex(filtered.length > 0 ? 0 : -1);
  }, [filtered.length]);

  if (!open) return null;

  const activeCommand = filtered[activeIndex];
  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (trapTabKey(event, dialogRef.current)) return;
    if (event.target !== inputRef.current) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex(nextEnabledIndex(activeIndex, filtered.length, 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex(nextEnabledIndex(activeIndex, filtered.length, -1));
      return;
    }
    if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(filtered.length > 0 ? 0 : -1);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      setActiveIndex(filtered.length - 1);
      return;
    }
    if (event.key === "Enter" && activeCommand) {
      event.preventDefault();
      activeCommand.run(context);
      onClose();
    }
  };

  const handleBackdrop = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  return (
    <div
      className="overlay-backdrop"
      data-testid="command-palette"
      onMouseDown={handleBackdrop}
    >
      <div
        ref={dialogRef}
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-labelledby="command-palette-title"
        onKeyDown={handleKeyDown}
      >
        <div className="command-palette-heading">
          <div>
            <p className="eyebrow">Local actions</p>
            <h2 id="command-palette-title">Command palette</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="Close command palette"
            onClick={onClose}
          >
            Close
          </button>
        </div>
        <label className="sr-only" htmlFor="command-palette-input">
          Search commands
        </label>
        <input
          ref={inputRef}
          id="command-palette-input"
          className="command-palette-input"
          type="text"
          value={query}
          placeholder="Search layout and focus actions"
          autoComplete="off"
          spellCheck={false}
          role="combobox"
          aria-expanded="true"
          aria-autocomplete="list"
          aria-controls="command-palette-list"
          aria-activedescendant={
            activeCommand ? `command-${activeIndex}` : undefined
          }
          onChange={(event) => setQuery(event.target.value)}
        />
        <div
          id="command-palette-list"
          className="command-list"
          role="listbox"
          aria-label="Available commands"
        >
          {filtered.length === 0 ? (
            <p className="command-empty">
              No matching commands. Only local layout and focus actions are
              available.
            </p>
          ) : (
            filtered.map((command, index) => (
              <button
                key={command.id}
                id={`command-${index}`}
                type="button"
                role="option"
                aria-selected={index === activeIndex}
                className="command-option"
                data-command-id={command.id}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => {
                  command.run(context);
                  onClose();
                }}
              >
                <span>
                  <strong>{command.label}</strong>
                  <small>{command.description}</small>
                </span>
                <span className="command-group">{command.group}</span>
              </button>
            ))
          )}
        </div>
        <p className="command-footer">
          <kbd>↑</kbd> <kbd>↓</kbd> navigate · <kbd>Enter</kbd> run ·{" "}
          <kbd>Esc</kbd> close
        </p>
      </div>
    </div>
  );
}
