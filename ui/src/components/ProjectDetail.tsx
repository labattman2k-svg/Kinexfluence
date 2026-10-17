import { useEffect, useState } from "react";
import { getProject, type Project } from "../lib/api";

interface Props {
  /** A project id or slug — the API accepts either. */
  idOrSlug: string;
  onBack: () => void;
}

export function ProjectDetail({ idOrSlug, onBack }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    getProject(idOrSlug)
      .then((found) => {
        if (cancelled) return;
        setProject(found);
        setError(null);
      })
      .catch((cause: Error) => {
        if (!cancelled) setError(cause.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [idOrSlug]);

  return (
    <section className="kfx-panel">
      <div className="kfx-panel-head">
        <h2>{project ? project.name : "Project"}</h2>
        <button type="button" onClick={onBack}>
          Back
        </button>
      </div>

      {error && (
        <p className="kfx-error" role="alert">
          {error}
        </p>
      )}

      {loading && <p className="kfx-muted">Loading…</p>}

      {project && (
        <dl className="kfx-detail">
          <dt>ID</dt>
          <dd className="kfx-mono">{project.id}</dd>

          <dt>Slug</dt>
          <dd className="kfx-mono">{project.slug}</dd>

          <dt>Status</dt>
          <dd>
            <span className={`kfx-status kfx-status-${project.status}`}>
              {project.status}
            </span>
          </dd>

          <dt>Description</dt>
          <dd>{project.description ?? <span className="kfx-muted">—</span>}</dd>

          <dt>Brief</dt>
          <dd>{project.brief ?? <span className="kfx-muted">—</span>}</dd>

          <dt>Created</dt>
          <dd>{new Date(project.created_at).toLocaleString()}</dd>

          <dt>Updated</dt>
          <dd>{new Date(project.updated_at).toLocaleString()}</dd>
        </dl>
      )}
    </section>
  );
}
