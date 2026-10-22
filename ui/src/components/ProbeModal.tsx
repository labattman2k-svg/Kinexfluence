import { useState } from "react";
import { answerProbe, submitProbe, type Probe } from "../lib/api";

/**
 * The four questions every probe asks.
 *
 * The same four, in the same order, for every probe. The point of the
 * mechanism is that the answers are comparable across snippets: one set of
 * questions each time means a user who has answered a dozen probes can see
 * their own reasoning, and the stored answers can be compared with each
 * other rather than with a different question each time.
 *
 * The order matches the backend's columns: `intent`, `impact`, `rationale`,
 * `confidence`. The field is named after what the answer is for; the
 * question is what the user is asked.
 */
export const PROBE_FIELDS = [
  { key: "intent", label: "Intent", question: "What is this?" },
  { key: "impact", label: "Impact", question: "What does it change or rule out?" },
  {
    key: "rationale",
    label: "Rationale",
    question: "How does it connect to the project?",
  },
  { key: "confidence", label: "Confidence", question: "How settled is it?" },
] as const;

/** The field a probe answer is written to. */
export type ProbeFieldKey = (typeof PROBE_FIELDS)[number]["key"];

/** The answers, keyed by the field they belong to. */
export type ProbeAnswers = Record<ProbeFieldKey, string>;

/**
 * What the modal is about.
 *
 * Either a snippet the user is claiming matters, or a probe that is already
 * waiting for an answer. The two are different jobs: the first creates the
 * probe and the answer together, the second answers one that exists.
 */
export type ProbeTarget =
  | { kind: "snippet"; snippetId: string; text: string; score: number }
  | { kind: "pending"; probe: Probe };

/** The trigger recorded for a probe the UI raises. */
const TRIGGER = "user_force_check";

/** The status that settles a probe and puts the floor under it. */
const SETTLED = "settled";

/** Empty answers, for a probe that has not been answered yet. */
function emptyAnswers(): ProbeAnswers {
  return { intent: "", impact: "", rationale: "", confidence: "" };
}

/** The answers of a probe already in the queue, as the modal shows them. */
function existingAnswers(probe: Probe): ProbeAnswers {
  return {
    intent: probe.intent ?? "",
    impact: probe.impact ?? "",
    rationale: probe.rationale ?? "",
    confidence: probe.confidence ?? "",
  };
}

/** A trimmed answer, or nothing at all for one that was left blank. */
function orNothing(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
}

interface Props {
  projectId: string;
  target: ProbeTarget;
  onClose: () => void;
  onAnswered: () => void;
}

/**
 * The probe modal: the question the user is asked when the scorer says a
 * snippet does not matter and the user says it does.
 *
 * The four fields map one to one onto the backend's `intent`, `impact`,
 * `rationale`, and `confidence` columns, and onto the `answers` array, so
 * the same answer is stored in both the general form and the indexed one.
 * A client that wants the four fields reads the columns; a client that wants
 * the questions with their answers reads the arrays.
 */
export function ProbeModal({ projectId, target, onClose, onAnswered }: Props) {
  const [answers, setAnswers] = useState<ProbeAnswers>(() =>
    target.kind === "pending" ? existingAnswers(target.probe) : emptyAnswers()
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A probe with no answer is a question the user has not asked themselves,
  // so at least one field has to be filled before the probe is worth
  // recording.
  const answered = PROBE_FIELDS.some((field) => answers[field.key].trim() !== "");

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!answered) return;

    setSaving(true);
    setError(null);

    try {
      if (target.kind === "snippet") {
        await submitProbe({
          snippet_id: target.snippetId,
          project_id: projectId,
          trigger: TRIGGER,
          questions: PROBE_FIELDS.map((field) => field.question),
          answers: PROBE_FIELDS.map((field) => answers[field.key].trim()),
          intent: orNothing(answers.intent),
          impact: orNothing(answers.impact),
          rationale: orNothing(answers.rationale),
          confidence: orNothing(answers.confidence),
          status: SETTLED,
        });
      } else {
        const probe = target.probe;

        // The answers array lines up with the questions that were asked. A
        // probe raised elsewhere may have asked a different number of them,
        // and an answer that does not line up is refused by the server
        // rather than stored against the wrong question. The named columns
        // carry the answer either way.
        const lines =
          probe.questions.length === PROBE_FIELDS.length
            ? PROBE_FIELDS.map((field) => answers[field.key].trim())
            : undefined;

        await answerProbe(probe.id, {
          answers: lines,
          intent: orNothing(answers.intent),
          impact: orNothing(answers.impact),
          rationale: orNothing(answers.rationale),
          confidence: orNothing(answers.confidence),
          status: SETTLED,
        });
      }

      onAnswered();
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setSaving(false);
    }
  };

  const answering = target.kind === "pending";

  return (
    <div className="kfx-modal-backdrop" onClick={onClose}>
      <div
        className="kfx-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="kfx-probe-title"
        onClick={(event) => event.stopPropagation()}
      >
        <h3 id="kfx-probe-title">
          {answering ? "Answer this probe" : "Why does this matter?"}
        </h3>
        <p className="kfx-modal-lead">
          {answering
            ? "The answer is what moves the snippet. The scorer will not argue with it."
            : "The scorer says this one does not matter. Tell it what you think it is."}
        </p>

        {target.kind === "snippet" ? (
          <p className="kfx-modal-snippet">{target.text.slice(0, 300)}</p>
        ) : (
          <ul className="kfx-modal-questions">
            {target.probe.questions.map((question) => (
              <li key={question}>{question}</li>
            ))}
          </ul>
        )}

        {error && (
          <p className="kfx-error" role="alert">
            {error}
          </p>
        )}

        <form className="kfx-form" onSubmit={handleSubmit}>
          {PROBE_FIELDS.map((field) => (
            <div key={field.key}>
              <label htmlFor={`kfx-probe-${field.key}`}>{field.label}</label>
              <textarea
                id={`kfx-probe-${field.key}`}
                value={answers[field.key]}
                placeholder={field.question}
                rows={2}
                onChange={(event) =>
                  setAnswers((current) => ({
                    ...current,
                    [field.key]: event.target.value,
                  }))
                }
              />
            </div>
          ))}

          <div className="kfx-modal-actions">
            <button type="button" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" disabled={!answered || saving}>
              {answering ? "Submit answer" : "Submit probe"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
