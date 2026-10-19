import { useEffect, useState } from "react";
import { listSnippets, getCart, addToCart, removeFromCart, type Snippet, type CartItem } from "../lib/api";

interface Props {
  projectId: string;
  cartReloadToken: number;
  onCartChanged: () => void;
}

export function SnippetList({ projectId, cartReloadToken, onCartChanged }: Props) {
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Fetch snippets on mount
  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    listSnippets()
      .then((found) => {
        if (cancelled) return;
        setSnippets(found);
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
  }, []);

  // Fetch cart when project or reload token changes
  useEffect(() => {
    let cancelled = false;

    getCart(projectId)
      .then((items) => {
        if (cancelled) return;
        setCartItems(items);
      })
      .catch((cause: Error) => {
        console.error("Failed to fetch cart:", cause);
      });

    return () => {
      cancelled = true;
    };
  }, [projectId, cartReloadToken]);

  const handleToggle = async (snippetId: string) => {
    const cartItem = cartItems.find((item) => item.snippet_id === snippetId);

    if (cartItem) {
      await removeFromCart(projectId, cartItem.item_id);
    } else {
      await addToCart(projectId, [snippetId]);
    }

    // Trigger a cart reload by calling onCartChanged
    onCartChanged();

    // Refetch cart locally to update checkboxes
    const updated = await getCart(projectId);
    setCartItems(updated);
  };

  const isInCart = (snippetId: string) => {
    return cartItems.some((item) => item.snippet_id === snippetId);
  };

  return (
    <section className="kfx-snippet-list">
      <div className="kfx-panel-head">
        <h2>Snippets</h2>
      </div>

      {error && (
        <p className="kfx-error" role="alert">
          {error}
        </p>
      )}

      {loading && <p className="kfx-muted">Loading…</p>}

      {!loading && snippets.length === 0 && (
        <p className="kfx-muted">No snippets imported yet.</p>
      )}

      {!loading && snippets.length > 0 && (
        <ul className="kfx-list">
          {snippets.map((snippet) => (
            <li key={snippet.id} className="kfx-list-item">
              <input
                type="checkbox"
                checked={isInCart(snippet.id)}
                onChange={() => handleToggle(snippet.id)}
                id={`snippet-${snippet.id}`}
              />
              <label htmlFor={`snippet-${snippet.id}`} className="kfx-list-content">
                <div className="kfx-snippet-kind">{snippet.kind}</div>
                <div className="kfx-snippet-text">
                  {snippet.text.slice(0, 200)}
                  {snippet.text.length > 200 && "…"}
                </div>
              </label>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
