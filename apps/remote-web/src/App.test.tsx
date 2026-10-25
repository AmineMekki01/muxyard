import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";

import { App } from "./App";

it("does not imply that remote access or privileged actions exist", () => {
  const html = renderToStaticMarkup(<App />);
  expect(html).toContain("Remote access is not enabled");
  expect(html).toContain("No host connection");
  expect(html).not.toMatch(/<(button|form|input|iframe)\b/);
});
