import { useEffect, useState } from "react";
import { getCart, removeFromCart, type CartItem } from "../lib/api";

interface Props {
  projectId: string;
  cartReloadToken: number;
  /** Bumped by the parent so this list refetches after a mutation. */
  onCartChanged: () => void;
  onBack: () => void;
}

export function CartSidebar({ projectId, cartReloadToken, onCartChanged, onBack }: Props) {
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
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
                <div className="kfx-snippet-kind">{item.kind}</div>
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
