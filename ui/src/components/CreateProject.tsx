import { useState, type FormEvent } from "react";
import { createProject } from "../lib/api";

interface Props {
  /** Called after a successful create, so the parent can return to the list. */
  onCreated: () => void;
  onCancel: () => void;
}

export function CreateProject({ onCreated, onCancel }: Props) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [brief, setBrief] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);

    try {
      await createProject({
        name,
        // The API takes Option<String>; sending an empty string would store an
        // empty string rather than nothing, so blank fields are omitted.
        description: description.trim() === "" ? undefined : description,
        brief: brief.trim() === "" ? undefined : brief,
      });
      onCreated();
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <section className="kfx-panel">
      <div className="kfx-panel-head">
        <h2>New project</h2>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>

      {error && (
        <p className="kfx-error" role="alert">
          {error}
        </p>
      )}

      <form onSubmit={handleSubmit} className="kfx-form">
        <label htmlFor="project-name">Name</label>
        <input
          id="project-name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Kilimo Data Residency"
          required
        />

        <label htmlFor="project-description">Description</label>
        <textarea
          id="project-description"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          rows={2}
          placeholder="What this project is about."
        />

        <label htmlFor="project-brief">Brief</label>
        <textarea
          id="project-brief"
          value={brief}
          onChange={(event) => setBrief(event.target.value)}
          rows={3}
          placeholder="Why it exists and what finished looks like."
        />

        <div className="kfx-form-actions">
          <button type="submit" disabled={submitting || name.trim() === ""}>
            {submitting ? "Creating…" : "Create project"}
          </button>
        </div>
      </form>
    </section>
  );
}
