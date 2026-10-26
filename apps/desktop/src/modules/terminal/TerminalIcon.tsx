/** Small inline icons for the terminal chrome; no external icon dependency. */

export type TerminalIconName =
  | "plus"
  | "menu"
  | "search"
  | "copy"
  | "close"
  | "split-right"
  | "split-down"
  | "maximize"
  | "restore";

type TerminalIconProps = {
  name: TerminalIconName;
};

export function TerminalIcon({ name }: TerminalIconProps) {
  return (
    <svg
      className="terminal-icon"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {name === "plus" ? (
        <>
          <path d="M8 3v10" />
          <path d="M3 8h10" />
        </>
      ) : null}
      {name === "menu" ? (
        <>
          <circle cx="3.5" cy="8" r="0.8" fill="currentColor" stroke="none" />
          <circle cx="8" cy="8" r="0.8" fill="currentColor" stroke="none" />
          <circle cx="12.5" cy="8" r="0.8" fill="currentColor" stroke="none" />
        </>
      ) : null}
      {name === "search" ? (
        <>
          <circle cx="7" cy="7" r="4" />
          <path d="m10 10 3 3" />
        </>
      ) : null}
      {name === "copy" ? (
        <>
          <rect x="5.5" y="5.5" width="7" height="8" rx="1.4" />
          <path d="M10.5 5.5V4.2c0-.7-.5-1.2-1.2-1.2H4.2C3.5 3 3 3.5 3 4.2v6.1c0 .7.5 1.2 1.2 1.2h1.3" />
        </>
      ) : null}
      {name === "close" ? (
        <>
          <path d="m4 4 8 8" />
          <path d="m12 4-8 8" />
        </>
      ) : null}
      {name === "split-right" ? (
        <>
          <rect x="2.5" y="3" width="11" height="10" rx="1.3" />
          <path d="M8 3v10" />
        </>
      ) : null}
      {name === "split-down" ? (
        <>
          <rect x="2.5" y="3" width="11" height="10" rx="1.3" />
          <path d="M2.5 8h11" />
        </>
      ) : null}
      {name === "maximize" ? (
        <>
          <path d="M6 3H3v3" />
          <path d="M10 13h3V10" />
          <path d="M13 6V3h-3" />
          <path d="M3 10v3h3" />
        </>
      ) : null}
      {name === "restore" ? (
        <>
          <rect x="5" y="5" width="8" height="8" rx="1.2" />
          <path d="M3 10V4.2C3 3.5 3.5 3 4.2 3H10" />
        </>
      ) : null}
    </svg>
  );
}
