import { describe, expect, it, vi } from "vitest";

import {
  createLinkProvider,
  findValidatedLinks,
  validateLinkUrl,
} from "./links";

describe("terminal links", () => {
  it("finds and trims http and https links", () => {
    expect(
      findValidatedLinks(
        "see https://example.com/docs, then http://localhost:3000.",
      ),
    ).toEqual([
      { url: "https://example.com/docs", start: 4, length: 24 },
      { url: "http://localhost:3000/", start: 35, length: 21 },
    ]);
  });

  it("rejects privileged and malformed URLs", () => {
    expect(validateLinkUrl("javascript:alert(1)")).toBeNull();
    expect(validateLinkUrl("data:text/html,hello")).toBeNull();
    expect(validateLinkUrl("file:///tmp/secret")).toBeNull();
    expect(validateLinkUrl("https://user:pass@example.com")).toBeNull();
    expect(validateLinkUrl("not a url")).toBeNull();
  });

  it("returns no links for malicious or overlong text", () => {
    expect(findValidatedLinks("javascript:alert(1)")).toEqual([]);
    expect(
      findValidatedLinks(`https://example.com/${"x".repeat(2100)}`),
    ).toEqual([]);
  });

  it("provides only validated links to xterm and opens them safely", () => {
    const open = vi.fn();
    const provider = createLinkProvider(
      {
        buffer: {
          active: {
            getLine: () => ({
              translateToString: () =>
                "https://example.com/path and javascript:alert(1)",
            }),
          },
        },
      } as never,
      open,
    );
    const callback = vi.fn();
    provider.provideLinks(1, callback);
    const links = callback.mock.calls[0]?.[0];
    expect(links).toHaveLength(1);
    expect(links?.[0]?.text).toBe("https://example.com/path");
    links?.[0]?.activate({} as MouseEvent, "https://example.com/path");
    expect(open).toHaveBeenCalledWith("https://example.com/path");
    links?.[0]?.activate({} as MouseEvent, "javascript:alert(1)");
    expect(open).toHaveBeenCalledTimes(1);
  });
});
