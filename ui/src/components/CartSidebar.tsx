import { useEffect, useMemo, useState } from "react";
import {
  getCart,
  removeFromCart,
  listScoredSnippets,
  type CartItem,
  type ScoredSnippet,
} from "../lib/api";
import { bandFor, scoreBands, BAND_GLYPH, BAND_LABEL } from "./SnippetList";

interface Props {
  projectId: string;
  cartReloadToken: number;
  /** Bumped by the parent so this list refetches after a mutation. */
  onCartChanged: () => void;
  onBack: () => void;
}

export function CartSidebar({ projectId, cartReloadToken, onCartChanged, onBack }: Props) {
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
  const [scored, setScored] = useState<ScoredSnippet[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    getCart(projectId)
      .then((items) => {
        if (cancelled) return;
        setCartItems(items);
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
  }, [projectId, cartReloadToken]);

  /**
   * The scored snippets, for the badges beside each cart item.
   *
   * The cart API does not carry a score, and a cart item is a materialised
   * copy of a snippet rather than the snippet itself, so the score has to be
   * matched back through `snippet_id`. A cart item with no snippet behind it
   * -- an item made by hand rather than from the browser -- simply has no
   * badge.
   */
  useEffect(() => {
    let cancelled = false;

    listScoredSnippets(projectId)
      .then((found) => {
        if (!cancelled) setScored(found);
      })
      .catch((cause: Error) => {
        // A missing score is not worth an error banner: the cart is still
        // perfectly usable without the badges.
        console.error("Failed to fetch scored snippets for the cart:", cause);
      });

    return () => {
      cancelled = true;
    };
  }, [projectId]);

  const bands = useMemo(() => scoreBands(scored), [scored]);

  const scoreBySnippetId = useMemo(() => {
    const map = new Map<string, number>();
    for (const snippet of scored) map.set(snippet.id, snippet.score);
    return map;
  }, [scored]);

  const handleRemove = async (itemId: string) => {
    try {
      await removeFromCart(projectId, itemId);
    } catch (cause) {
      setError((cause as Error).message);
      return;
    }
    // The parent bumps `cartReloadToken`, which re-runs the effect above. Doing
    // it locally instead would leave `SnippetList`'s checkboxes stale, since it
    // keeps its own copy of the cart.
    onCartChanged();
  };

  return (
    <aside className="kfx-cart-sidebar">
      <div className="kfx-panel-head">
        <h2>Cart</h2>
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

      {!loading && cartItems.length === 0 && (
        <p className="kfx-muted">Cart is empty.</p>
      )}

      {!loading && cartItems.length > 0 && (
        <ul className="kfx-list">
          {cartItems.map((item) => {
            const score =
              item.snippet_id !== null
                ? scoreBySnippetId.get(item.snippet_id)
                : undefined;
            const band = score !== undefined ? bandFor(score, bands) : null;

            return (
              <li key={item.item_id} className="kfx-list-item">
                <div className="kfx-cart-item-content">
                  <div className="kfx-snippet-head">
                    <div className="kfx-snippet-kind">{item.kind}</div>
                    {band !== null && score !== undefined && (
                      <span
                        className={`kfx-score-badge kfx-score-${band}`}
                        data-testid="cart-score-badge"
                      >
                        <span className="kfx-score-glyph" aria-hidden="true">
                          {BAND_GLYPH[band]}
                        </span>
                        {score.toFixed(2)}
                        <span className="kfx-sr-only">{` — ${BAND_LABEL[band]}`}</span>
                      </span>
                    )}
                  </div>
                  <div className="kfx-snippet-text">
                    {item.text.slice(0, 100)}
                    {item.text.length > 100 && "…"}
                  </div>
                </div>
                <button
                  type="button"
                  className="kfx-remove-btn"
                  onClick={() => handleRemove(item.item_id)}
                  aria-label="Remove from cart"
                >
                  ×
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}
