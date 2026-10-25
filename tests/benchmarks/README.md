# Benchmark Fixtures

`fixtures/ps-macos.txt` is a deterministic process-table sample used by the
performance harness tests. It covers a desktop root, WebKit helper processes, a
shell child, and no persistent host process.

The executable harness lives in `scripts/perf`. Reports are written as JSON and
may also be rendered as Markdown:

```sh
pnpm build:host
pnpm build:desktop
node scripts/perf/baseline.mjs \
  --iterations 5 \
  --idle-ms 3000 \
  --output target/perf/baseline.json \
  --markdown target/perf/baseline.md
```

Unavailable measurements are reported as `NOT_AVAILABLE` with a reason. The
current persistent host runtime is not implemented, so host ready/idle metrics
are unavailable while host CLI startup is measured. macOS desktop readiness uses
an on-screen window probe; other platforms report desktop readiness as
unavailable. RSS is available on Unix and Windows; working-set and private-byte
metrics are Windows-specific.
