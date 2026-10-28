// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Project } from "@muxyard/contracts";

import { ProjectSidebar } from "./ProjectSidebar";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const project: Project = {
  id: "project-1",
  name: "Demo",
  root_path: "/tmp/demo",
  execution_environment: { kind: "native-mac" },
  repo_id: null,
  trust: "untrusted",
  created_ms: 1n,
  last_opened_ms: 2n,
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  host?.remove();
  host = null;
});

describe("project sidebar", () => {
  it("lists projects, opens one, and removes one", async () => {
    const onOpen = vi.fn();
    const onRemove = vi.fn();
    await act(async () => {
      root?.render(
        <ProjectSidebar
          projects={[project]}
          activeProjectId="project-1"
          busy={false}
          error={null}
          onOpen={onOpen}
          onRegister={vi.fn()}
          onRemove={onRemove}
        />,
      );
    });

    const open = document.querySelector<HTMLButtonElement>(".project-open");
    expect(open?.textContent).toContain("Demo");
    expect(open?.getAttribute("aria-current")).toBe("page");
    await act(async () => open?.click());
    expect(onOpen).toHaveBeenCalledWith(project);

    const remove = document.querySelector<HTMLButtonElement>(
      '[aria-label="Remove project Demo"]',
    );
    await act(async () => remove?.click());
    expect(onRemove).toHaveBeenCalledWith(project);
  });

  it("registers a directory path and reports errors", async () => {
    const onRegister = vi.fn();
    await act(async () => {
      root?.render(
        <ProjectSidebar
          projects={[]}
          activeProjectId={null}
          busy={false}
          error="Directory missing"
          onOpen={vi.fn()}
          onRegister={onRegister}
          onRemove={vi.fn()}
        />,
      );
    });
    expect(host?.textContent).toContain("No projects registered");
    expect(host?.textContent).toContain("Directory missing");

    const input = document.querySelector<HTMLInputElement>("#project-path");
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "/tmp/new-project");
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      input?.form?.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
    });
    expect(onRegister).toHaveBeenCalledWith("/tmp/new-project");
  });
});
