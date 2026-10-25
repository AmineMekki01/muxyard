/** Host connection status surfaces; no terminal or remote authority is implied. */

import type { HostViewState } from "../ipc/host";
import { hostStatusLabel } from "../ipc/host";

export function HostStatusPill({ state }: { state: HostViewState }) {
  const detail =
    state.kind === "offline" || state.kind === "incompatible"
      ? state.error.message
      : undefined;
  return (
    <span
      className="status-pill status-pill-host"
      data-host-state={state.kind}
      data-testid="host-status"
      title={detail}
    >
      {hostStatusLabel(state)}
    </span>
  );
}

export function HostBanner({
  state,
  onReconnect,
}: {
  state: HostViewState;
  onReconnect: () => void;
}) {
  if (state.kind !== "offline" && state.kind !== "incompatible") {
    return null;
  }
  const incompatible = state.kind === "incompatible";
  return (
    <div className="host-banner" role="alert" data-testid="host-error">
      <div className="host-banner-copy">
        <strong>{incompatible ? "Host incompatible" : "Host offline"}</strong>
        <p>{state.error.message}</p>
      </div>
      <button type="button" className="secondary-button" onClick={onReconnect}>
        Reconnect
      </button>
    </div>
  );
}
