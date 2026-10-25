/** Verify deterministic summary statistics. */
import assert from "node:assert/strict";
import test from "node:test";
import { round, summarize } from "./stats.mjs";

test("summarizes odd and even sample counts", () => {
  assert.deepEqual(summarize([10]), {
    count: 1,
    min_ms: 10,
    median_ms: 10,
    p95_ms: 10,
    max_ms: 10,
    mean_ms: 10,
  });
  const summary = summarize([1, 2, 3, 4]);
  assert.equal(summary.median_ms, 2.5);
  assert.equal(summary.p95_ms, 4);
  assert.equal(summary.mean_ms, 2.5);
});

test("empty samples are unavailable rather than zero", () => {
  assert.equal(summarize([]), null);
  assert.equal(summarize(null), null);
});

test("rounding does not turn missing values into zero", () => {
  assert.equal(round(Number.NaN), null);
  assert.equal(round(Number.POSITIVE_INFINITY), null);
  assert.equal(round(1.23456, 2), 1.23);
});
