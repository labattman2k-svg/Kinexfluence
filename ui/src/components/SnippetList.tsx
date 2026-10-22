import { useEffect, useMemo, useRef, useState } from "react";
import {
  listScoredSnippets,
  getCart,
  addToCart,
  getPendingProbes,
  type ScoredSnippet,
  type CartItem,
  type Probe,
} from "../lib/api";
import { ProbeModal, type ProbeTarget } from "./ProbeModal";

interface Props {
  projectId: string;
  cartReloadToken: number;
  onCartChanged: () => void;
}

/**
 * The nominal band edges: 0.70 is a strong candidate and is auto-checked,
 * 0.40 is worth a look, and below that is not.
 *
 * The score they are applied to is relative, not absolute. It is a sigmoid
 * over six weighted signals, and three of them -- code, decision markers,
 * and project entities -- depend on what the brief says and what the
 * conversation contains. A project whose snippets hit none of those cannot
 * score above 0.26 however good it is, so painting green at 0.70 would paint
 * nothing green and auto-select nothing. Measured against the shipped
 * fixture, the whole distribution sits between 0.06 and 0.25.
 *
 * So the bands scale with the list -- green is the top slice of what is on
 * screen, amber the slice below -- and the nominal edges cap both. When a
 * project's scores do approach the practical maximum, the scaled edges land
 * on the nominal ones and the two agree.
 */
const NOMINAL_GREEN = 0.7;
const NOMINAL_AMBER = 0.4;
const GREEN_SHARE = 0.85;
const AMBER_SHARE = 0.45;

/**
 * The floor under both scaled edges: a snippet below this scored nothing the
 * scorer could act on, so it stays weak rather than having the bands
 * stretched over nothing to distinguish.
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
 *
 * A snippet the user has floored is left out of the calculation. The floor
 * is the user's override of the scorer, not another opinion from it, and
 * letting a floored 0.90 into the calibration would push the green edge to
 * its nominal 0.70 and paint every other snippet in the list grey.
 */
export function scoreBands(snippets: ScoredSnippet[]): ScoreBands {
  const unfloored = snippets.filter((s) => s.score_details.probe_floor === null);
  const calibration = unfloored.length > 0 ? unfloored : snippets;
  const best = calibration.reduce((max, snippet) => Math.max(max, snippet.score), 0);
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

/** Props for {@link ScoreBadge}. */
interface ScoreBadgeProps {
  score: number;
  band: ScoreBand;
  testId?: string;
}

/**
 * The score badge: the number, the band colour, and a glyph.
 *
 * The glyph and the screen-reader label are what carry the band for anyone who
 * cannot see the colour, which the accessibility standard requires -- colour
 * alone is never the signal.
 */
export function ScoreBadge({ score, band, testId }: ScoreBadgeProps) {
  return (
    <span className={`kfx-score-badge kfx-score-${band}`} data-testid={testId} data-band={band}>
      <span className="kfx-score-glyph" aria-hidden="true">
        {BAND_GLYPH[band]}
      </span>
      {score.toFixed(2)}
      <span className="kfx-sr-only">{` — ${BAND_LABEL[band]}`}</span>
    </span>
  );
}

/** A copy of a set with one member taken out. */
function without<T>(member: T): (current: Set<T>) => Set<T> {
  return (current) => {
    const next = new Set(current);
    next.delete(member);
    return next;
  };
}

/**
 * The signals shown in the breakdown, in the order a reader wants them.
 *
 * The keys are named rather than derived from `keyof ScoreDetails`, because
 * that type also covers `score` and `probe_floor`, which are not signals and
 * are not all numbers.
 */
const BREAKDOWN: { key: "code" | "marker" | "entity" | "position"; label: string }[] = [
  { key: "code", label: "Code" },
  { key: "marker", label: "Markers" },
  { key: "entity", label: "Entities" },
  { key: "position", label: "Position" },
];

export function SnippetList({ projectId, cartReloadToken, onCartChanged }: Props) {
  const [snippets, setSnippets] = useState<ScoredSnippet[]>([]);
  const [cartItems, setCartItems] = useState<CartItem[]>([]);
  // `selected` holds the auto-selected high band plus the user's ticks, and
  // `dismissed` the snippets the user unticked or took out of the cart. The
  // two are separate so a later refetch -- the cart arriving, say -- cannot
  // quietly undo a choice the user made.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [pinnedId, setPinnedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The probes in this project that are still waiting for an answer.
  const [pendingProbes, setPendingProbes] = useState<Probe[]>([]);
  // The probe the user is being asked about, if the modal is open.
  const [probeTarget, setProbeTarget] = useState<ProbeTarget | null>(null);
  // Bumped when a probe is answered, so the queue is fetched again.
  const [probeReloadToken, setProbeReloadToken] = useState(0);

  // The cart membership as of the last fetch, in a ref rather than state
  // because it is only ever compared against the next fetch to work out what
  // left the cart -- it is not rendered.
  const previousCartIds = useRef<Set<string>>(new Set());

  /**
   * Apply a fresh list of scored snippets.
   *
   * The scores, the cleared error, and the auto-selection are decided
   * together: the high band starts checked, so a user who trusts the score
   * has only to confirm rather than pick.
   */
  const applyScored = (found: ScoredSnippet[]) => {
    setSnippets(found);
    setError(null);
    const next = scoreBands(found);
    setSelected(new Set(found.filter((s) => s.score >= next.green).map((s) => s.id)));
  };

  // Fetch scored snippets on mount
  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    listScoredSnippets(projectId)
      .then((found) => {
        if (cancelled) return;
        applyScored(found);
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

  /**
   * Re-read the scores: a probe puts a floor under one, so the list would
   * otherwise keep showing the number the scorer gave.
   */
  const reloadScored = async () => {
    try {
      applyScored(await listScoredSnippets(projectId));
    } catch (cause) {
      setError((cause as Error).message);
    }
  };

  // Fetch the probes still waiting for an answer
  useEffect(() => {
    let cancelled = false;

    getPendingProbes(projectId)
      .then((probes) => {
        if (!cancelled) setPendingProbes(probes);
      })
      .catch((cause: Error) => {
        if (!cancelled) console.error("Failed to fetch pending probes:", cause);
      });

    return () => {
      cancelled = true;
    };
  }, [projectId, probeReloadToken]);

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

  // Which snippets are ticked: the selection, plus whatever is already in the
  // cart, minus the dismissed ones. The cart is in the union so the box never
  // claims a snippet is not in the cart when it is.
  const selectedIds = useMemo(() => {
    const ticked = new Set([...selected, ...inCart]);
    for (const id of dismissed) ticked.delete(id);
    return ticked;
  }, [selected, inCart, dismissed]);

  // The ticked snippets that are not in the cart yet. Adding is idempotent on
  // the server, so re-adding is harmless, but counting only the new ones
  // keeps the button honest about what it will change.
  const pendingIds = useMemo(
    () => [...selectedIds].filter((id) => !inCart.has(id)),
    [selectedIds, inCart]
  );

  const handleToggle = (snippet: ScoredSnippet) => {
    const isSelected = selectedIds.has(snippet.id);

    if (isSelected) {
      setSelected(without(snippet.id));
      setDismissed((current) => new Set([...current, snippet.id]));
      return;
    }

    // The scorer and the user disagreeing is the case the probe mechanism
    // exists for. A snippet the scorer thinks is worth nothing is the one
    // the user is most likely to be right about, so the tick is turned into
    // a question rather than taken at face value.
    if (bandFor(snippet.score, bands) === "low") {
      setProbeTarget({
        kind: "snippet",
        snippetId: snippet.id,
        text: snippet.text,
        score: snippet.score,
      });
      return;
    }

    setDismissed(without(snippet.id));
    setSelected((current) => new Set([...current, snippet.id]));
  };

  /**
   * The text of the snippet a pending probe is about: a probe names an item
   * rather than a snippet, and puts the item in the cart, so the cart holds
   * the text.
   */
  const queueText = (probe: Probe) => {
    const item = cartItems.find((entry) => entry.item_id === probe.item_id);
    if (!item) return probe.trigger;
    return item.text.slice(0, 160) + (item.text.length > 160 ? "…" : "");
  };

  /**
   * What happens once a probe has been answered: the scores are read again
   * because the floor has moved one of them, the cart is reloaded because the
   * answered item is now in it, and the queue is fetched again because the
   * probe is no longer waiting.
   */
  const handleProbeAnswered = () => {
    setProbeTarget(null);
    setProbeReloadToken((token) => token + 1);
    onCartChanged();
    void reloadScored();
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

      {pendingProbes.length > 0 && (
        <div className="kfx-probe-queue" data-testid="probe-queue">
          <h3 className="kfx-probe-queue-title">
            {pendingProbes.length} probe{pendingProbes.length === 1 ? "" : "s"} waiting
            for an answer
          </h3>
          <ul className="kfx-list">
            {pendingProbes.map((probe) => (
              <li key={probe.id} className="kfx-list-item">
                <div className="kfx-probe-queue-item">
                  <span className="kfx-probe-queue-text">{queueText(probe)}</span>
                  <button
                    type="button"
                    onClick={() => setProbeTarget({ kind: "pending", probe })}
                  >
                    Answer
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}

      {loading && <p className="kfx-muted">Loading…</p>}

      {!loading && snippets.length === 0 && (
        <p className="kfx-muted">No snippets imported yet.</p>
      )}

      {!loading && snippets.length > 0 && (
        <ul className="kfx-list">
          {snippets.map((snippet) => {
            const band = bandFor(snippet.score, bands);
            const probeFloor = snippet.score_details.probe_floor;
            const panelId = `kfx-score-panel-${snippet.id}`;
            const panelOpen =
              hoverId === snippet.id || focusId === snippet.id || pinnedId === snippet.id;

            return (
              <li key={snippet.id} className="kfx-list-item">
                <input
                  type="checkbox"
                  checked={selectedIds.has(snippet.id)}
                  onChange={() => handleToggle(snippet)}
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
                    <ScoreBadge score={snippet.score} band={band} testId="score-badge" />
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
                      {probeFloor !== null && (
                        <div className="kfx-score-row">
                          <dt>Probe floor</dt>
                          <dd>{probeFloor.toFixed(2)}</dd>
                        </div>
                      )}
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

      {probeTarget && (
        <ProbeModal
          projectId={projectId}
          target={probeTarget}
          onClose={() => setProbeTarget(null)}
          onAnswered={handleProbeAnswered}
        />
      )}
    </section>
  );
}
