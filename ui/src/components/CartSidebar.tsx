import { useEffect, useState } from "react";
import {
  getCart,
  removeFromCart,
  listScoredSnippets,
  type CartItem,
  type ScoredSnippet,
} from "../lib/api";
import { bandFor, scoreBands, ScoreBadge, type ScoreBand } from "./SnippetList";

interface Props {
  projectId: string;
  cartReloadToken: number;
  /** Bumped by the parent so this list refetches after a mutation. */
  onCartChanged: () => void;
  onBack: () => void;
}

/** A cart item's score, matched back to the snippet it came from. */
interface ScoreEntry {
  score: number;
  band: ScoreBand;
}

export function CartSidebar({ projectId, cartReloadToken, onCartChanged, onBack }: Props) {
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
  const [scores, setScores] = useState<Map<string, ScoreEntry>>(new Map());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    // The cart API does not carry a score, and a cart item is a materialised
    // copy of a snippet rather than the snippet itself, so the score has to be
    // matched back through `snippet_id`. A cart item with no snippet behind
    // it -- an item made by hand rather than from the browser -- simply has
    // no badge.
    const scored = listScoredSnippets(projectId).catch((cause: Error) => {
      // A missing score is not worth an error banner: the cart is still
      // perfectly usable without the badges.
      console.error("Failed to fetch scored snippets for the cart:", cause);
      return [] as ScoredSnippet[];
    });

    Promise.all([getCart(projectId), scored])
      .then(([items, found]) => {
        if (cancelled) return;
        setCartItems(items);
        setScores(scoresBySnippet(found));
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
          {cartItems.map((item) => (
            <li key={item.item_id} className="kfx-list-item">
              <div className="kfx-cart-item-content">
                <div className="kfx-snippet-head">
                  <div className="kfx-snippet-kind">{item.kind}</div>
                  <CartScoreBadge entry={scores.get(item.snippet_id ?? "")} />
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
          ))}
        </ul>
      )}
    </aside>
  );
}

/** The score badge for a cart item, if it has one. */
function CartScoreBadge({ entry }: { entry: ScoreEntry | undefined }) {
  if (!entry) return null;
  return <ScoreBadge score={entry.score} band={entry.band} testId="cart-score-badge" />;
}

/** Map each snippet to its score and band, using the list's own bands. */
function scoresBySnippet(scored: ScoredSnippet[]): Map<string, ScoreEntry> {
  const bands = scoreBands(scored);
  const map = new Map<string, ScoreEntry>();
  for (const snippet of scored) {
    map.set(snippet.id, { score: snippet.score, band: bandFor(snippet.score, bands) });
  }
  return map;
}
