import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { App } from "./App";

describe("foundation workspace", () => {
  it("renders an original empty workspace with honest connection state", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain("<h1>No project open</h1>");
    expect(html).toContain("Host not connected");
    expect(html).toContain("Development scaffold");
    expect(html).toContain("not connected yet");
  });

  it("offers no pretend terminal, project, or remote actions", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).not.toMatch(/<(button|input|iframe|canvas)\b/);
  });
});
