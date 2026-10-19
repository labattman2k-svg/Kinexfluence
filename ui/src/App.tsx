import { useState } from "react";
import { CreateProject } from "./components/CreateProject";
import { ProjectDetail } from "./components/ProjectDetail";
import { ProjectList } from "./components/ProjectList";
import { SnippetList } from "./components/SnippetList";
import { CartSidebar } from "./components/CartSidebar";

/**
 * Which view is showing.
 *
 * A union rather than three booleans, so "detail with no project selected"
 * cannot be represented.
 */
type View =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "detail"; idOrSlug: string }
  | { kind: "cart"; projectId: string };

export function App() {
  const [view, setView] = useState<View>({ kind: "list" });
  // Bumped after a create so the list refetches instead of showing a table
  // that is missing the project just added.
  const [reloadToken, setReloadToken] = useState(0);
  // Bumped when the cart is mutated so SnippetList checkboxes stay in sync.
  const [cartReloadToken, setCartReloadToken] = useState(0);

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
          onOpenCart={(projectId) => setView({ kind: "cart", projectId })}
        />
      )}

      {view.kind === "cart" && (
        <div className="kfx-cart-view">
          <SnippetList
            projectId={view.projectId}
            cartReloadToken={cartReloadToken}
            onCartChanged={() => setCartReloadToken((token) => token + 1)}
          />
          <CartSidebar
            projectId={view.projectId}
            cartReloadToken={cartReloadToken}
            onCartChanged={() => setCartReloadToken((token) => token + 1)}
            onBack={() => setView({ kind: "list" })}
          />
        </div>
      )}
    </main>
  );
}
