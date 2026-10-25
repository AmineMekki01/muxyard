import { describe, expect, it } from "vitest";

import type { HostStatusResponse } from "@muxyard/contracts";

import {
  applyHostEvent,
  errorToHostState,
  hostStatusDetail,
  hostStatusLabel,
} from "./host";

const status: HostStatusResponse = {
  host_version: "0.1.0",
  protocol_version: 1,
  instance_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
  pid: 42,
  process_start: "start",
  started_ms: 1,
  ready: true,
};

describe("host bridge state", () => {
  it("turns a snapshot into a connected state", () => {
    expect(
      applyHostEvent({ kind: "connecting" }, { kind: "snapshot", status }),
    ).toEqual({
      kind: "connected",
      status,
    });
  });

  it("keeps the last status during bounded reconnect attempts", () => {
    const connected = { kind: "connected" as const, status };
    expect(
      applyHostEvent(connected, { kind: "reconnecting", attempt: 2 }),
    ).toEqual({
      kind: "reconnecting",
      attempt: 2,
      status,
    });
  });

  it("maps incompatible and offline errors to visible states", () => {
    const connected = { kind: "connected" as const, status };
    expect(
      applyHostEvent(connected, {
        kind: "offline",
        error: {
          code: "UNSUPPORTED_CAPABILITY",
          message: "Host protocol is incompatible.",
        },
      }),
    ).toMatchObject({
      kind: "incompatible",
      error: { code: "UNSUPPORTED_CAPABILITY" },
      status,
    });

    expect(
      errorToHostState({
        code: "HOST_OFFLINE",
        message: "Host is offline.",
      }),
    ).toMatchObject({ kind: "offline" });
    expect(errorToHostState(new Error("bridge unavailable"))).toMatchObject({
      kind: "offline",
      error: { code: "INTERNAL" },
    });
  });

  it("provides honest labels and details", () => {
    expect(hostStatusLabel({ kind: "connecting" })).toBe("Host starting");
    expect(hostStatusLabel({ kind: "connected", status })).toBe("Host ready");
    expect(hostStatusLabel({ kind: "reconnecting", attempt: 1 })).toBe(
      "Host reconnecting",
    );
    expect(
      hostStatusLabel({
        kind: "offline",
        error: { code: "HOST_OFFLINE", message: "offline" },
      }),
    ).toBe("Host offline");
    expect(hostStatusDetail({ kind: "connected", status })).toContain(
      "12345678",
    );
  });
});
