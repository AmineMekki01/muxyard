/** Keyboard reference for nonterminal shell navigation. */

import {
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";

import { SHORTCUTS } from "../app/keymap";
import { trapTabKey, useDialogFocus } from "./dialogFocus";

type ShortcutHelpProps = {
  open: boolean;
  onClose: () => void;
};

export function ShortcutHelp({ open, onClose }: ShortcutHelpProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useDialogFocus(open, closeRef);

  if (!open) return null;

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    trapTabKey(event, dialogRef.current);
  };

  const handleBackdrop = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  return (
    <div
      className="overlay-backdrop"
      data-testid="shortcut-help"
      onMouseDown={handleBackdrop}
    >
      <div
        ref={dialogRef}
        className="shortcut-help"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcut-help-title"
        onKeyDown={handleKeyDown}
      >
        <div className="command-palette-heading">
          <div>
            <p className="eyebrow">Reference</p>
            <h2 id="shortcut-help-title">Keyboard shortcuts</h2>
          </div>
          <button
            ref={closeRef}
            type="button"
            className="icon-button"
            aria-label="Close keyboard shortcuts"
            onClick={onClose}
          >
            Close
          </button>
        </div>
        <dl className="shortcut-list">
          {SHORTCUTS.map((shortcut) => (
            <div key={shortcut.keys} className="shortcut-row">
              <dt>
                <kbd>{shortcut.keys}</kbd>
              </dt>
              <dd>
                <strong>{shortcut.label}</strong>
                <span>{shortcut.description}</span>
              </dd>
            </div>
          ))}
        </dl>
        <p className="scope-note">
          Terminal tabs, splits, and pane navigation use Cmd/Ctrl+Shift
          shortcuts. Ctrl+C, Ctrl+D, Ctrl+Z, Escape, mouse reporting, and Alt
          sequences are left to the focused terminal. Text fields keep their
          normal editing shortcuts.
        </p>
      </div>
    </div>
  );
}
