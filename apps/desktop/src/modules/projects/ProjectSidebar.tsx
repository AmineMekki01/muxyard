import { useState, type FormEvent } from "react";

import type { Project } from "@muxyard/contracts";

type ProjectSidebarProps = {
  projects: Project[];
  activeProjectId: string | null;
  busy: boolean;
  error: string | null;
  onOpen: (project: Project) => void;
  onRegister: (path: string) => void;
  onRemove: (project: Project) => void;
};

export function ProjectSidebar({
  projects,
  activeProjectId,
  busy,
  error,
  onOpen,
  onRegister,
  onRemove,
}: ProjectSidebarProps) {
  const [path, setPath] = useState("");

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const value = path.trim();
    if (!value || busy) return;
    onRegister(value);
    setPath("");
  };

  return (
    <section className="sidebar-section project-section">
      <h3>Projects</h3>
      {projects.length === 0 ? (
        <p>No projects registered.</p>
      ) : (
        <ul className="project-list">
          {projects.map((project) => (
            <li key={project.id}>
              <button
                type="button"
                className="project-open"
                data-active={project.id === activeProjectId}
                aria-current={
                  project.id === activeProjectId ? "page" : undefined
                }
                onClick={() => onOpen(project)}
              >
                <strong>{project.name}</strong>
                <span>{project.root_path}</span>
              </button>
              <button
                type="button"
                className="project-remove"
                aria-label={`Remove project ${project.name}`}
                title="Remove project"
                disabled={busy}
                onClick={() => onRemove(project)}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      <form className="project-register" onSubmit={submit}>
        <label htmlFor="project-path">Open directory</label>
        <input
          id="project-path"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="/path/to/project"
          disabled={busy}
          autoComplete="off"
        />
        <button
          type="submit"
          className="shell-button"
          disabled={busy || !path.trim()}
        >
          Open
        </button>
      </form>
      {error ? (
        <p className="project-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
