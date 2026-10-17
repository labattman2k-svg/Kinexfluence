import { useEffect, useState } from "react";
import { listProjects, type Project } from "../lib/api";

interface Props {
  /** Open the detail view for a project. */
  onOpen: (project: Project) => void;
  /** Switch to the create form. */
  onCreate: () => void;
  /** Bumped by the parent to force a refetch, e.g. after a create. */
  reloadToken: number;
}

export function ProjectList({ onOpen, onCreate, reloadToken }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    listProjects()
      .then((found) => {
        if (cancelled) return;
        setProjects(found);
        setError(null);
      })
      .catch((cause: Error) => {
        if (!cancelled) setError(cause.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    // Without this, a response that lands after the view is gone would set
    // state on an unmounted component.
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  return (
    <section className="kfx-panel">
      <div className="kfx-panel-head">
        <h2>Projects</h2>
        <button type="button" onClick={onCreate}>
          New project
        </button>
      </div>

      {error && (
        <p className="kfx-error" role="alert">
          {error}
        </p>
      )}

      {loading && <p className="kfx-muted">Loading…</p>}

      {!loading && !error && projects.length === 0 && (
        <p className="kfx-muted">No projects yet. Create one to get started.</p>
      )}

      {projects.length > 0 && (
        <table className="kfx-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Slug</th>
              <th>Status</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {projects.map((project) => (
              <tr
                key={project.id}
                onClick={() => onOpen(project)}
                className="kfx-row"
              >
                <td>
                  {/* The row click is the convenience; this button is the
                      affordance a keyboard and a screen reader can use. */}
                  <button
                    type="button"
                    className="kfx-link"
                    onClick={() => onOpen(project)}
                  >
                    {project.name}
                  </button>
                </td>
                <td className="kfx-mono">{project.slug}</td>
                <td>
                  <span className={`kfx-status kfx-status-${project.status}`}>
                    {project.status}
                  </span>
                </td>
                <td className="kfx-muted">
                  {new Date(project.updated_at).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
