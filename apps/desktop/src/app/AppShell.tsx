/** Accessible workspace shell with local-only layout and focus actions. */

import { useEffect, useRef, useState } from "react";

import type { Project } from "@muxyard/contracts";

import { useHostConnection } from "../ipc/useHostConnection";
import { hostStatusDetail } from "../ipc/host";
import {
  listProjects,
  registerProject,
  removeProject,
} from "../modules/projects/projects";
import { ProjectSidebar } from "../modules/projects/ProjectSidebar";
import {
  TerminalWorkspace,
  type TerminalWorkspaceHandle,
} from "../modules/terminal/TerminalWorkspace";
import { CommandPalette } from "../ui/CommandPalette";
import { HostBanner, HostStatusPill } from "../ui/HostStatus";
import { ShortcutHelp } from "../ui/ShortcutHelp";
import { createCommands, type CommandContext } from "./commands";
import {
  isHelpShortcut,
  isPaletteShortcut,
  shouldIgnoreGlobalShortcut,
} from "./keymap";
import { useFocusManager } from "./useFocusManager";

const COMMANDS = createCommands();

export function AppShell() {
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [projects, setProjects] = useState<Project[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectError, setProjectError] = useState<string | null>(null);
  const terminalRef = useRef<TerminalWorkspaceHandle>(null);
  const { state: hostState, reconnect } = useHostConnection();
  const activeProject =
    projects.find((project) => project.id === activeProjectId) ?? null;
  const {
    activeRegion,
    setActiveRegion,
    focusRegion,
    sidebarOpen,
    inspectorOpen,
    toggleSidebar,
    toggleInspector,
    sidebarRef,
    mainRef,
    inspectorRef,
  } = useFocusManager();

  const context: CommandContext = {
    toggleSidebar,
    toggleInspector,
    focusRegion,
    openHelp: () => setHelpOpen(true),
    newTerminalTab: () => terminalRef.current?.newTab(),
    splitTerminal: (direction) => terminalRef.current?.splitActive(direction),
    toggleTerminalZoom: () => terminalRef.current?.toggleZoom(),
    closeTerminalPane: () => terminalRef.current?.requestCloseActivePane(),
    closeOtherTerminalTabs: () => terminalRef.current?.requestCloseOtherTabs(),
    moveActiveTerminalTab: (delta) => terminalRef.current?.moveActiveTab(delta),
    nextTerminalTab: () => terminalRef.current?.nextTab(),
    previousTerminalTab: () => terminalRef.current?.previousTab(),
    nextTerminalPane: () => terminalRef.current?.nextPane(),
    previousTerminalPane: () => terminalRef.current?.previousPane(),
    renameActiveTerminalTab: () => terminalRef.current?.renameActiveTab(),
  };

  useEffect(() => {
    if (hostState.kind !== "connected") return;
    let alive = true;
    setProjectBusy(true);
    void listProjects()
      .then((response) => {
        if (!alive) return;
        setProjects(response.projects);
        setActiveProjectId((current) =>
          current && response.projects.some((project) => project.id === current)
            ? current
            : (response.projects[0]?.id ?? null),
        );
        setProjectError(null);
      })
      .catch((reason: unknown) => {
        if (alive) {
          setProjectError(
            reason instanceof Error
              ? reason.message
              : "Projects could not be loaded.",
          );
        }
      })
      .finally(() => {
        if (alive) setProjectBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [hostState.kind]);

  const openProject = (project: Project) => {
    setActiveProjectId(project.id);
    void registerProject({ path: project.root_path, name: project.name })
      .then((updated) => {
        setProjects((current) => [
          updated,
          ...current.filter((item) => item.id !== updated.id),
        ]);
        setProjectError(null);
      })
      .catch((reason: unknown) => {
        setProjectError(
          reason instanceof Error
            ? reason.message
            : "The project could not be opened.",
        );
      });
  };

  const addProject = (path: string) => {
    setProjectBusy(true);
    void registerProject({ path, name: null })
      .then((project) => {
        setProjects((current) => [
          project,
          ...current.filter((item) => item.id !== project.id),
        ]);
        setActiveProjectId(project.id);
        setProjectError(null);
      })
      .catch((reason: unknown) => {
        setProjectError(
          reason instanceof Error
            ? reason.message
            : "The project directory could not be opened.",
        );
      })
      .finally(() => setProjectBusy(false));
  };

  const forgetProject = (project: Project) => {
    setProjectBusy(true);
    void removeProject({ id: project.id })
      .then(() => {
        setProjects((current) =>
          current.filter((item) => item.id !== project.id),
        );
        setActiveProjectId((current) =>
          current === project.id ? null : current,
        );
        setProjectError(null);
      })
      .catch((reason: unknown) => {
        setProjectError(
          reason instanceof Error
            ? reason.message
            : "The project could not be removed.",
        );
      })
      .finally(() => setProjectBusy(false));
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (paletteOpen || helpOpen) return;
      if (shouldIgnoreGlobalShortcut(event.target)) return;
      if (isPaletteShortcut(event)) {
        event.preventDefault();
        setPaletteOpen(true);
      } else if (isHelpShortcut(event)) {
        event.preventDefault();
        setHelpOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [paletteOpen, helpOpen]);

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-group">
          <span className="wordmark">
            <span className="yard-mark" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            Muxyard
          </span>
          <span className="breadcrumb">
            Workspace / {activeProject?.name ?? "No project"}
          </span>
        </div>
        <div className="topbar-actions">
          <button
            type="button"
            className="shell-button"
            data-testid="toggle-sidebar"
            aria-expanded={sidebarOpen}
            aria-controls="workspace-sidebar"
            aria-label={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
            onClick={toggleSidebar}
          >
            Sidebar
          </button>
          <button
            type="button"
            className="shell-button"
            data-testid="toggle-inspector"
            aria-expanded={inspectorOpen}
            aria-controls="workspace-inspector"
            aria-label={inspectorOpen ? "Hide inspector" : "Show inspector"}
            onClick={toggleInspector}
          >
            Inspector
          </button>
          <button
            type="button"
            className="palette-trigger"
            data-testid="command-palette-trigger"
            aria-haspopup="dialog"
            aria-expanded={paletteOpen}
            aria-keyshortcuts="Control+Shift+P Meta+Shift+P"
            onClick={() => setPaletteOpen(true)}
          >
            Command palette <kbd>Cmd/Ctrl+Shift+P</kbd>
          </button>
          <HostStatusPill state={hostState} />
          <span className="status-pill status-pill-quiet">Remote off</span>
        </div>
      </header>

      <div
        className="workspace-grid"
        data-sidebar={sidebarOpen ? "open" : "closed"}
        data-inspector={inspectorOpen ? "open" : "closed"}
      >
        {sidebarOpen ? (
          <aside
            id="workspace-sidebar"
            ref={sidebarRef}
            className="sidebar"
            tabIndex={-1}
            aria-label="Workspace sidebar"
            data-region={activeRegion === "sidebar" ? "active" : undefined}
            onFocus={() => setActiveRegion("sidebar")}
          >
            <div className="panel-heading">
              <p className="eyebrow">Workspace</p>
              <h2>Projects</h2>
            </div>
            <nav aria-label="Workspace sections">
              <ProjectSidebar
                projects={projects}
                activeProjectId={activeProjectId}
                busy={projectBusy}
                error={projectError}
                onOpen={openProject}
                onRegister={addProject}
                onRemove={forgetProject}
              />
              <section className="sidebar-section">
                <h3>Tasks</h3>
                <p>No tasks yet.</p>
              </section>
              <section className="sidebar-section">
                <h3>Sessions</h3>
                <p>No sessions yet.</p>
              </section>
              <section className="sidebar-section">
                <h3>Attention</h3>
                <p>No pending attention.</p>
              </section>
            </nav>
            <section className="sidebar-section keyboard-hint">
              <h3>Keyboard</h3>
              <p>
                <kbd>Cmd/Ctrl+Shift+P</kbd> opens local commands. <kbd>?</kbd>{" "}
                shows shortcuts.
              </p>
              <button
                type="button"
                className="text-button"
                onClick={() => setHelpOpen(true)}
              >
                Show keyboard shortcuts
              </button>
            </section>
          </aside>
        ) : null}

        <main
          id="workspace-main"
          ref={mainRef}
          className="main-workspace"
          tabIndex={-1}
          aria-label="Main workspace"
          data-region={activeRegion === "main" ? "active" : undefined}
          onFocus={() => setActiveRegion("main")}
        >
          <HostBanner state={hostState} onReconnect={reconnect} />
          {hostState.kind === "connected" ||
          hostState.kind === "reconnecting" ? (
            <TerminalWorkspace
              ref={terminalRef}
              shortcutsEnabled={!paletteOpen && !helpOpen}
            />
          ) : (
            <div className="empty-content">
              <p className="eyebrow">Your workspace</p>
              <h1>No project open</h1>
              <p className="empty-description">
                A clear space for your next build.
              </p>
              <div className="empty-actions">
                <button
                  type="button"
                  className="primary-button"
                  onClick={() => setPaletteOpen(true)}
                >
                  Open command palette
                </button>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => setHelpOpen(true)}
                >
                  Keyboard shortcuts
                </button>
              </div>
              <p className="scope-note">
                Project registration is available in the sidebar. Agents and
                remote access are not connected yet. The local terminal is
                available when the host is ready.
              </p>
            </div>
          )}
        </main>

        {inspectorOpen ? (
          <aside
            id="workspace-inspector"
            ref={inspectorRef}
            className="inspector"
            tabIndex={-1}
            aria-label="Inspector"
            data-region={activeRegion === "inspector" ? "active" : undefined}
            onFocus={() => setActiveRegion("inspector")}
          >
            <div className="panel-heading">
              <p className="eyebrow">Inspector</p>
              <h2>Task details</h2>
            </div>
            <section className="inspector-section">
              <h3>Changes</h3>
              <p>No task selected.</p>
            </section>
            <section className="inspector-section">
              <h3>Verification</h3>
              <p>Not run.</p>
            </section>
            <section className="inspector-section">
              <h3>Review notes</h3>
              <p>No review yet.</p>
            </section>
            <section className="inspector-section">
              <h3>Task metadata</h3>
              <dl>
                <div>
                  <dt>State</dt>
                  <dd>Not selected</dd>
                </div>
                <div>
                  <dt>Worktree</dt>
                  <dd>Not created</dd>
                </div>
              </dl>
            </section>
          </aside>
        ) : null}
      </div>

      <footer className="statusbar">
        <span>Local application</span>
        <span>{sidebarOpen ? "Sidebar open" : "Sidebar hidden"}</span>
        <span>{inspectorOpen ? "Inspector open" : "Inspector hidden"}</span>
        <span>{hostStatusDetail(hostState)}</span>
        <span>Remote off</span>
      </footer>

      <p className="sr-only" aria-live="polite">
        {paletteOpen
          ? "Command palette open"
          : helpOpen
            ? "Keyboard shortcuts open"
            : ""}
      </p>

      <CommandPalette
        open={paletteOpen}
        commands={COMMANDS}
        context={context}
        onClose={() => setPaletteOpen(false)}
      />
      <ShortcutHelp open={helpOpen} onClose={() => setHelpOpen(false)} />
    </div>
  );
}
