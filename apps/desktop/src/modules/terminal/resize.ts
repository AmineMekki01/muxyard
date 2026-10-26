/** Coalesces resize-observer bursts before fitting xterm and notifying the host. */

export type ResizeScheduler = {
  schedule: () => void;
  cancel: () => void;
};

export function createResizeScheduler(
  fit: () => void,
  delayMs = 50,
): ResizeScheduler {
  let timer: ReturnType<typeof setTimeout> | null = null;

  return {
    schedule() {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fit();
      }, delayMs);
    },
    cancel() {
      if (timer === null) return;
      clearTimeout(timer);
      timer = null;
    },
  };
}
