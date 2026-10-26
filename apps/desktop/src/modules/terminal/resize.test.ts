import { afterEach, describe, expect, it, vi } from "vitest";

import { createResizeScheduler } from "./resize";

describe("resize scheduler", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces resize bursts into one fit", () => {
    vi.useFakeTimers();
    const fit = vi.fn();
    const scheduler = createResizeScheduler(fit, 50);
    scheduler.schedule();
    scheduler.schedule();
    scheduler.schedule();
    expect(fit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(49);
    expect(fit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fit).toHaveBeenCalledTimes(1);
  });

  it("cancels a pending fit on unmount", () => {
    vi.useFakeTimers();
    const fit = vi.fn();
    const scheduler = createResizeScheduler(fit, 50);
    scheduler.schedule();
    scheduler.cancel();
    vi.advanceTimersByTime(100);
    expect(fit).not.toHaveBeenCalled();
  });
});
