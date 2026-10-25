/** Typed desktop-side host connection state and bridge calls. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ApiError,
  HostStatusEvent,
  HostStatusResponse,
} from "@muxyard/contracts";

export type HostViewState =
  | { kind: "connecting"; status?: HostStatusResponse | undefined }
  | { kind: "connected"; status: HostStatusResponse }
  | {
      kind: "reconnecting";
      attempt: number;
      status?: HostStatusResponse | undefined;
    }
  | {
      kind: "offline";
      error: ApiError;
      status?: HostStatusResponse | undefined;
    }
  | {
      kind: "incompatible";
      error: ApiError;
      status?: HostStatusResponse | undefined;
    };

export function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}

export function errorToHostState(
  error: unknown,
  status?: HostStatusResponse,
): HostViewState {
  const apiError: ApiError = isApiError(error)
    ? error
    : {
        code: "INTERNAL",
        message: "The desktop host bridge is unavailable.",
      };
  if (
    apiError.code === "UNSUPPORTED_CAPABILITY" ||
    apiError.code === "UNSUPPORTED_VERSION"
  ) {
    return { kind: "incompatible", error: apiError, status };
  }
  return { kind: "offline", error: apiError, status };
}

export function applyHostEvent(
  previous: HostViewState,
  event: HostStatusEvent,
): HostViewState {
  switch (event.kind) {
    case "snapshot":
    case "status":
      return { kind: "connected", status: event.status };
    case "reconnecting":
      return {
        kind: "reconnecting",
        attempt: event.attempt,
        status: previous.status,
      };
    case "offline":
      return errorToHostState(event.error, previous.status);
  }
}

export function hostStatusLabel(state: HostViewState): string {
  switch (state.kind) {
    case "connecting":
      return "Host starting";
    case "connected":
      return "Host ready";
    case "reconnecting":
      return "Host reconnecting";
    case "offline":
      return "Host offline";
    case "incompatible":
      return "Host incompatible";
  }
}

export function hostStatusDetail(state: HostViewState): string {
  if (state.kind === "connected") {
    return `Host ${state.status.instance_id.slice(0, 8)} · v${state.status.host_version}`;
  }
  if (state.kind === "reconnecting") {
    return `Host reconnecting (attempt ${state.attempt})`;
  }
  if (state.kind === "offline" || state.kind === "incompatible") {
    return state.error.message;
  }
  return "Connecting to local host";
}

export async function connectHost(): Promise<HostStatusResponse> {
  return invoke<HostStatusResponse>("host_connect");
}

export async function startHostWatch(
  onEvent: (event: HostStatusEvent) => void,
): Promise<() => void> {
  const channel = new Channel<HostStatusEvent>();
  channel.onmessage = onEvent;
  await invoke("host_watch", { channel });
  return () => {
    void invoke("host_unwatch").catch(() => {});
  };
}
