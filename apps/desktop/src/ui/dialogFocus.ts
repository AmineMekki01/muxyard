/** Focus restoration and Tab trapping shared by lightweight dialogs. */

import { useEffect, type RefObject } from "react";

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export function useDialogFocus<T extends HTMLElement>(
  open: boolean,
  initialFocusRef: RefObject<T | null>,
): void {
  useEffect(() => {
    if (!open) return;
    const previous =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    initialFocusRef.current?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [open, initialFocusRef]);
}

export function trapTabKey(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "preventDefault">,
  container: HTMLElement | null,
): boolean {
  if (event.key !== "Tab" || !container) return false;
  const focusable = Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE),
  ).filter(
    (element) =>
      !element.hasAttribute("disabled") &&
      element.getAttribute("aria-hidden") !== "true",
  );
  if (focusable.length === 0) return false;

  const first = focusable[0];
  const last = focusable.at(-1);
  if (!first || !last) return false;
  const active = document.activeElement;
  const activeIsFocusable =
    active instanceof HTMLElement && focusable.includes(active);

  if (event.shiftKey && (!activeIsFocusable || active === first)) {
    event.preventDefault();
    last.focus();
    return true;
  }
  if (!event.shiftKey && (!activeIsFocusable || active === last)) {
    event.preventDefault();
    first.focus();
    return true;
  }
  return false;
}
