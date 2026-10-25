/** Small deterministic statistics helpers for benchmark reports. */

export function summarize(samples) {
  if (!Array.isArray(samples) || samples.length === 0) return null;
  const sorted = [...samples].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  const median =
    sorted.length % 2 === 0
      ? (sorted[middle - 1] + sorted[middle]) / 2
      : sorted[middle];
  const p95Index = Math.max(0, Math.ceil(sorted.length * 0.95) - 1);
  const sum = sorted.reduce((total, value) => total + value, 0);
  return {
    count: sorted.length,
    min_ms: round(sorted[0]),
    median_ms: round(median),
    p95_ms: round(sorted[p95Index]),
    max_ms: round(sorted.at(-1)),
    mean_ms: round(sum / sorted.length),
  };
}

export function round(value, digits = 3) {
  if (!Number.isFinite(value)) return null;
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}
