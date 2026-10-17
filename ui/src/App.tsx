import { useState } from "react";
import { CreateProject } from "./components/CreateProject";
import { ProjectDetail } from "./components/ProjectDetail";
import { ProjectList } from "./components/ProjectList";

/**
 * Which view is showing.
 *
 * A union rather than three booleans, so "detail with no project selected"
 * cannot be represented.
 */
type View =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "detail"; idOrSlug: string };

export function App() {
  const [view, setView] = useState<View>({ kind: "list" });
  // Bumped after a create so the list refetches instead of showing a table
  // that is missing the project just added.
  const [reloadToken, setReloadToken] = useState(0);

  return (
    <main className="kfx-app">
      <header>
        <h1>Kinexfluence</h1>
        <p className="tagline">
          Where scattered thinking becomes project intelligence.
        </p>
      </header>

      {view.kind === "list" && (
        <ProjectList
          reloadToken={reloadToken}
          onCreate={() => setView({ kind: "create" })}
          onOpen={(project) => setView({ kind: "detail", idOrSlug: project.id })}
        />
      )}

      {view.kind === "create" && (
        <CreateProject
          onCancel={() => setView({ kind: "list" })}
          onCreated={() => {
            setReloadToken((token) => token + 1);
            setView({ kind: "list" });
          }}
        />
      )}

      {view.kind === "detail" && (
        <ProjectDetail
          idOrSlug={view.idOrSlug}
          onBack={() => setView({ kind: "list" })}
        />
      )}
    </main>
  );
}
