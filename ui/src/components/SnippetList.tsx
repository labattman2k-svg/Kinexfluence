import { useEffect, useMemo, useRef, useState } from "react";
import {
  listScoredSnippets,
  getCart,
  addToCart,
  type ScoredSnippet,
  type CartItem,
} from "../lib/api";

interface Props {
  projectId: string;
  cartReloadToken: number;
  onCartChanged: () => void;
}

/**
 * The nominal band edges.
 *
 * A snippet at 0.70 or above is a strong candidate and gets auto-checked; one
 * at 0.40 is worth a look; one below that is not.
 */
const NOMINAL_GREEN = 0.7;
const NOMINAL_AMBER = 0.4;

/**
 * The share of the best score in the list that counts as green, and as amber.
 *
 * The nominal edges are absolute, but the score they are applied to is not.
 * The score is a sigmoid over six weighted signals, and three of them --
 * code, decision markers, and project entities -- depend on what the brief
 * happens to say and what the conversation happens to contain. A project
 * whose snippets hit none of those signals cannot score above 0.26, however
 * good it is, so painting green at 0.70 would paint nothing green and
 * auto-select nothing. Measured against the shipped fixture, the whole
 * distribution sits between 0.06 and 0.25.
 *
 * So the bands scale with the list: green is the top slice of what is on
 * screen, amber the slice below it, and the nominal edges cap both. When a
 * project's snippets do approach the practical maximum of the curve, the
 * scaled edges land on the nominal ones and the two agree.
 */
const GREEN_SHARE = 0.85;
const AMBER_SHARE = 0.45;

/**
 * The floor under both scaled edges.
 *
 * A snippet below this scored nothing the scorer could act on, so it stays
 * weak rather than having the bands stretched over nothing to distinguish.
 */
const MIN_SIGNAL = 0.1;

/** The band a score falls in. */
export type ScoreBand = "high" | "medium" | "low";

/** The band edges for one list of snippets. */
export interface ScoreBands {
  /** At or above this the snippet is green and auto-checked. */
  green: number;
  /** At or above this the snippet is amber. */
  amber: number;
}

/**
 * Work out the band edges for a list of scored snippets.
 *
 * Exported so the calibration is inspectable and testable rather than buried
 * in a component.
 */
export function scoreBands(snippets: ScoredSnippet[]): ScoreBands {
  const best = snippets.reduce((max, snippet) => Math.max(max, snippet.score), 0);
  return {
    green: Math.min(NOMINAL_GREEN, Math.max(MIN_SIGNAL, best * GREEN_SHARE)),
    amber: Math.min(NOMINAL_AMBER, Math.max(MIN_SIGNAL, best * AMBER_SHARE)),
  };
}

/** The band a single score falls in, given the list's edges. */
export function bandFor(score: number, bands: ScoreBands): ScoreBand {
  if (score >= bands.green) return "high";
  if (score >= bands.amber) return "medium";
  return "low";
}

/** The glyph that carries the band without relying on colour alone. */
export const BAND_GLYPH: Record<ScoreBand, string> = {
  high: "▲",
  medium: "◆",
  low: "▼",
};

/** The band named in words, for a screen reader. */
export const BAND_LABEL: Record<ScoreBand, string> = {
  high: "high relevance",
  medium: "medium relevance",
  low: "low relevance",
};

/** The signals shown in the breakdown, in the order a reader wants them. */
const BREAKDOWN: { key: keyof ScoredSnippet["score_details"]; label: string }[] = [
  { key: "code", label: "Code" },
  { key: "marker", label: "Markers" },
  { key: "entity", label: "Entities" },
  { key: "position", label: "Position" },
];

export function SnippetList({ projectId, cartReloadToken, onCartChanged }: Props) {
  const [snippets, setSnippets] = useState<ScoredSnippet[]>([]);
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
  // `selected` holds the auto-selected high band plus the user's ticks.
  // `dismissed` holds the snippets the user unticked, and the ones taken out
  // of the cart. The two are separate so that a later refetch -- the cart
  // arriving, say -- cannot quietly undo a choice the user made.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [pinnedId, setPinnedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /**
   * The cart membership as of the last fetch.
   *
   * Kept in a ref rather than state because it is only ever compared against
   * the next fetch to work out what left the cart -- it is not rendered.
   */
  const previousCartIds = useRef<Set<string>>(new Set());

  // Fetch scored snippets on mount
  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    listScoredSnippets(projectId)
      .then((found) => {
        if (cancelled) return;
        setSnippets(found);
        setError(null);

        // The high band starts checked, so a user who trusts the score has
        // only to confirm rather than pick. It is a default, not a decision:
        // the cart still waits for the button below.
        const bands = scoreBands(found);
        setSelected(new Set(found.filter((s) => s.score >= bands.green).map((s) => s.id)));
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
  }, [projectId]);

  // Fetch cart when project or reload token changes
  useEffect(() => {
    let cancelled = false;

    getCart(projectId)
      .then((items) => {
        if (cancelled) return;
        setCartItems(items);

        const currentIds = new Set(
          items.map((item) => item.snippet_id).filter((id): id is string => id !== null)
        );

        // A snippet taken out of the cart is no longer selected: the box
        // means "this is in the cart, or queued to be", and leaving it ticked
        // would claim something the user just threw away. The removal is
        // remembered, because the auto-selected set would otherwise put the
        // tick straight back.
        const removed = [...previousCartIds.current].filter((id) => !currentIds.has(id));
        previousCartIds.current = currentIds;

        if (removed.length > 0) {
          setSelected((current) => {
            const next = new Set(current);
            for (const id of removed) next.delete(id);
            return next;
          });
          setDismissed((current) => new Set([...current, ...removed]));
        }
      })
      .catch((cause: Error) => {
        console.error("Failed to fetch cart:", cause);
      });

    return () => {
      cancelled = true;
    };
  }, [projectId, cartReloadToken]);

  const bands = useMemo(() => scoreBands(snippets), [snippets]);

  const inCart = useMemo(
    () =>
      new Set(
        cartItems.map((item) => item.snippet_id).filter((id): id is string => id !== null)
      ),
    [cartItems]
  );

  /**
   * Which snippets are ticked.
   *
   * The selection, plus whatever is already in the cart, minus the snippets
   * that were dismissed. The cart is in the union so the box never claims a
   * snippet is not in the cart when it is.
   */
  const selectedIds = useMemo(() => {
    const ticked = new Set([...selected, ...inCart]);
    for (const id of dismissed) ticked.delete(id);
    return ticked;
  }, [selected, inCart, dismissed]);

  /**
   * The selected snippets that are not in the cart yet.
   *
   * Adding is idempotent on the server, so re-adding is harmless, but
   * counting only the new ones keeps the button honest about what it will
   * change.
   */
  const pendingIds = useMemo(
    () => [...selectedIds].filter((id) => !inCart.has(id)),
    [selectedIds, inCart]
  );

  const handleToggle = (snippetId: string) => {
    const isSelected = selectedIds.has(snippetId);

    if (isSelected) {
      setSelected((current) => {
        const next = new Set(current);
        next.delete(snippetId);
        return next;
      });
      setDismissed((current) => new Set([...current, snippetId]));
    } else {
      setDismissed((current) => {
        const next = new Set(current);
        next.delete(snippetId);
        return next;
      });
      setSelected((current) => new Set([...current, snippetId]));
    }
  };

  const handleAddToCart = async () => {
    if (pendingIds.length === 0) return;

    setAdding(true);
    try {
      await addToCart(projectId, pendingIds);
      onCartChanged();
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setAdding(false);
    }
  };

  return (
    <section className="kfx-snippet-list">
      <div className="kfx-panel-head">
        <h2>Snippets</h2>
        <div className="kfx-panel-actions">
          <button
            type="button"
            className="kfx-add-to-cart"
            onClick={handleAddToCart}
            disabled={pendingIds.length === 0 || adding}
          >
            Add {pendingIds.length} to Cart
          </button>
        </div>
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
          {snippets.map((snippet) => {
            const band = bandFor(snippet.score, bands);
            const panelId = `kfx-score-panel-${snippet.id}`;
            const panelOpen =
              hoverId === snippet.id || focusId === snippet.id || pinnedId === snippet.id;

            return (
              <li key={snippet.id} className="kfx-list-item">
                <input
                  type="checkbox"
                  checked={selectedIds.has(snippet.id)}
                  onChange={() => handleToggle(snippet.id)}
                  id={`snippet-${snippet.id}`}
                />
                <label htmlFor={`snippet-${snippet.id}`} className="kfx-list-content">
                  <div className="kfx-snippet-head">
                    <div className="kfx-snippet-kind">
                      {snippet.kind}
                      {inCart.has(snippet.id) && (
                        <span className="kfx-in-cart">in cart</span>
                      )}
                    </div>
                    <span
                      className={`kfx-score-badge kfx-score-${band}`}
                      data-testid="score-badge"
                      data-band={band}
                    >
                      <span className="kfx-score-glyph" aria-hidden="true">
                        {BAND_GLYPH[band]}
                      </span>
                      {snippet.score.toFixed(2)}
                      <span className="kfx-sr-only">{` — ${BAND_LABEL[band]}`}</span>
                    </span>
                  </div>
                  <div className="kfx-snippet-text">
                    {snippet.text.slice(0, 200)}
                    {snippet.text.length > 200 && "…"}
                  </div>
                </label>
                <div
                  className="kfx-score-info"
                  onMouseEnter={() => setHoverId(snippet.id)}
                  onMouseLeave={() => setHoverId(null)}
                >
                  <button
                    type="button"
                    className="kfx-score-info-btn"
                    aria-expanded={panelOpen}
                    aria-controls={panelId}
                    aria-label={`Why did this snippet score ${snippet.score.toFixed(2)}?`}
                    onClick={() =>
                      setPinnedId((current) => (current === snippet.id ? null : snippet.id))
                    }
                    onFocus={() => setFocusId(snippet.id)}
                    onBlur={() => setFocusId(null)}
                  >
                    i
                  </button>
                  <div
                    className={`kfx-score-panel${panelOpen ? " kfx-score-panel-open" : ""}`}
                    role="tooltip"
                    id={panelId}
                  >
                    <dl className="kfx-score-breakdown">
                      {BREAKDOWN.map(({ key, label }) => (
                        <div key={key} className="kfx-score-row">
                          <dt>{label}</dt>
                          <dd>{snippet.score_details[key].toFixed(2)}</dd>
                        </div>
                      ))}
                    </dl>
                    <p className="kfx-score-note">
                      Novelty and provenance are stubbed at 0.50 until semantic
                      similarity lands.
                    </p>
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
