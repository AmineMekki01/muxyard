/** Compact terminal action menu with keyboard navigation. */

import { useEffect, useRef, useState } from "react";

import { TerminalIcon, type TerminalIconName } from "./TerminalIcon";

export type TerminalMenuAction = {
  id: string;
  label: string;
  icon?: TerminalIconName;
  disabled?: boolean;
  danger?: boolean;
  onSelect: () => void;
};

type TerminalMenuProps = {
  actions: readonly TerminalMenuAction[];
};

export function TerminalMenu({ actions }: TerminalMenuProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const first = menuRef.current?.querySelector<HTMLButtonElement>(
      '[role="menuitem"]:not([disabled])',
    );
    first?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (rootRef.current?.contains(event.target)) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };

  const handleMenuKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    const items = Array.from(
      menuRef.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not([disabled])',
      ) ?? [],
    );
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    let next: HTMLButtonElement | undefined;
    if (event.key === "ArrowDown") {
      next = items[(index + 1 + items.length) % items.length];
    } else if (event.key === "ArrowUp") {
      next = items[(index - 1 + items.length) % items.length];
    } else if (event.key === "Home") {
      next = items[0];
    } else if (event.key === "End") {
      next = items.at(-1);
    }
    if (!next) return;
    event.preventDefault();
    next.focus();
  };

  return (
    <div className="terminal-menu" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className="terminal-icon-button"
        aria-label="Terminal actions"
        title="Terminal actions"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <TerminalIcon name="menu" />
      </button>
      {open ? (
        <div
          ref={menuRef}
          className="terminal-menu-popover"
          role="menu"
          aria-label="Terminal actions"
          onKeyDown={handleMenuKeyDown}
        >
          {actions.map((action) => (
            <button
              key={action.id}
              type="button"
              role="menuitem"
              className="terminal-menu-item"
              data-danger={action.danger ? "true" : "false"}
              disabled={action.disabled}
              onClick={() => {
                close();
                action.onSelect();
              }}
            >
              {action.icon ? <TerminalIcon name={action.icon} /> : null}
              <span>{action.label}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
