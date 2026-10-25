import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { HostStatusResponse } from "@muxyard/contracts";

import type { HostViewState } from "../ipc/host";
import { HostBanner, HostStatusPill } from "./HostStatus";

const status: HostStatusResponse = {
  host_version: "0.1.0",
  protocol_version: 1,
  instance_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
  pid: 42,
  process_start: "start",
  started_ms: 1,
  ready: true,
};

describe("host status surfaces", () => {
  it("renders connected and reconnecting labels", () => {
    expect(
      renderToStaticMarkup(
        <HostStatusPill state={{ kind: "connected", status }} />,
      ),
    ).toContain("Host ready");
    expect(
      renderToStaticMarkup(
        <HostStatusPill state={{ kind: "reconnecting", attempt: 2, status }} />,
      ),
    ).toContain("Host reconnecting");
  });

  it("renders offline and incompatible errors with a reconnect action", () => {
    const offline: HostViewState = {
      kind: "offline",
      error: { code: "HOST_OFFLINE", message: "Host is offline." },
    };
    const incompatible: HostViewState = {
      kind: "incompatible",
      error: {
        code: "UNSUPPORTED_CAPABILITY",
        message: "Host protocol is incompatible.",
      },
    };

    const offlineHtml = renderToStaticMarkup(
      <HostBanner state={offline} onReconnect={() => {}} />,
    );
    expect(offlineHtml).toContain("Host offline");
    expect(offlineHtml).toContain("Host is offline.");
    expect(offlineHtml).toContain("Reconnect");

    const incompatibleHtml = renderToStaticMarkup(
      <HostBanner state={incompatible} onReconnect={() => {}} />,
    );
    expect(incompatibleHtml).toContain("Host incompatible");
    expect(incompatibleHtml).toContain("Host protocol is incompatible.");
  });

  it("does not show a banner while connected", () => {
    expect(
      renderToStaticMarkup(
        <HostBanner
          state={{ kind: "connected", status }}
          onReconnect={() => {}}
        />,
      ),
    ).toBe("");
  });
});
