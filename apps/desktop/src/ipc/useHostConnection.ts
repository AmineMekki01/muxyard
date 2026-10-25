/** React lifecycle for the typed host bridge and its bounded status watcher. */

import { useCallback, useEffect, useRef, useState } from "react";

import type { HostViewState } from "./host";
import {
  applyHostEvent,
  connectHost,
  errorToHostState,
  startHostWatch,
} from "./host";

type StopWatch = () => void;

export function useHostConnection() {
  const [state, setState] = useState<HostViewState>({ kind: "connecting" });
  const runId = useRef(0);
  const stopWatchRef = useRef<StopWatch | null>(null);

  const stopWatch = useCallback(() => {
    stopWatchRef.current?.();
    stopWatchRef.current = null;
  }, []);

  const startWatch = useCallback(async (run: number) => {
    const stop = await startHostWatch((event) => {
      if (runId.current !== run) return;
      setState((previous) => applyHostEvent(previous, event));
    });
    if (runId.current !== run) {
      stop();
      return;
    }
    stopWatchRef.current?.();
    stopWatchRef.current = stop;
  }, []);

  const connect = useCallback(async () => {
    const run = ++runId.current;
    stopWatch();
    setState({ kind: "connecting" });
    try {
      const status = await connectHost();
      if (runId.current !== run) return;
      setState({ kind: "connected", status });
      await startWatch(run);
    } catch (error) {
      if (runId.current === run) {
        setState(errorToHostState(error));
      }
    }
  }, [startWatch, stopWatch]);

  useEffect(() => {
    void connect();
    return () => {
      runId.current += 1;
      stopWatch();
    };
  }, [connect, stopWatch]);

  return { state, reconnect: connect };
}
