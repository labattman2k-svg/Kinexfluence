import { expect, test } from "@playwright/test";

/**
 * The probe flow, driven the way a person does it.
 *
 * The scorer says a snippet does not matter. The user says it does, and
 * answers why. The answer puts a floor under the snippet's score, and the
 * list, the cart, and the API all agree afterwards.
 *
 * The floor is the whole point, so the test checks it against the API rather
 * than against the rendering: a badge that said 0.90 while the server scored
 * the snippet 0.08 would pass a visual test and fail the user.
 */

/**
 * The ports the core might be on, in the order to try them.
 *
 * The core binds the first free port from its configured starting point. The
 * Playwright config asks for 47899, but the core moves on if that port is
 * taken -- and `reuseExistingServer` means an already-running core on any
 * port could be answering instead. So the test probes the same range the UI
 * does, and takes whichever answers.
 */
const CANDIDATE_PORTS = [
  47899, 47821, 47822, 47823, 47824, 47825, 47826, 47827, 47828, 47829, 47830,
];

/** The port that answered, once one has. */
let cachedCoreUrl: string | null = null;

/** Find the core. */
async function fetchCore(): Promise<string> {
  if (cachedCoreUrl) return cachedCoreUrl;

  for (const port of CANDIDATE_PORTS) {
    const url = `http://127.0.0.1:${port}`;
    try {
      const response = await fetch(`${url}/v1/health`);
      const body = (await response.json()) as { status?: string };
      if (response.ok && body.status === "ok") {
        cachedCoreUrl = url;
        return url;
      }
    } catch {
      // Nothing listening on this port; try the next.
    }
  }

  throw new Error("Cannot reach the Kinexfluence core on any expected port.");
}

/**
 * The scorer's entity rules, as patterns.
 *
 * A Capitalised word of more than three characters, a camelCase word, or a
 * snake_case word. Any of these, taken from a snippet's own text, is one the
 * scorer extracts as an entity -- and the snippet therefore contains it, so
 * the entity match follows.
 */
const ENTITY_PATTERNS = [
  /^[A-Z][A-Za-z0-9]{3,}$/,
  /^[a-z][A-Za-z0-9]*[A-Z][A-Za-z0-9]*$/,
  /^(?=[A-Za-z0-9]*[A-Za-z])[a-z0-9]+(_[a-z0-9]+)+$/i,
];

/**
 * A brief guaranteed to match the snippets, whatever the fixture holds.
 *
 * The bands are relative to the list, so a list where every snippet scored
 * the same would have no low band and the test would have nothing to probe.
 * A brief derived from the snippets themselves keeps the entity signal
 * working, and therefore keeps the scores spread out.
 */
async function deriveBrief(): Promise<string> {
  const core = await fetchCore();
  const response = await fetch(`${core}/v1/snippets?limit=50&offset=0`);
  if (!response.ok) {
    throw new Error(`Could not list snippets: ${response.status}`);
  }

  const snippets = (await response.json()) as { text: string }[];
  if (snippets.length === 0) {
    throw new Error("The core holds no snippets, so the fixture import failed.");
  }

  const words = snippets
    .flatMap((snippet) => snippet.text.split(/\s+/))
    .map((word) => word.replace(/^[^A-Za-z0-9_]+|[^A-Za-z0-9_]+$/g, ""));

  const entity = words.find((word) => ENTITY_PATTERNS.some((p) => p.test(word)));
  if (entity) return entity;

  return words.find((word) => word.length > 3) ?? "brief";
}

/** A snippet as the scored-snippets endpoint renders it. */
interface ScoredSnippet {
  id: string;
  score: number;
  score_details: { probe_floor: number | null };
}

/** The project the test created, by name. */
async function findProject(core: string, name: string) {
  const projects = (await (await fetch(`${core}/v1/projects`)).json()) as {
    id: string;
    name: string;
  }[];
  const project = projects.find((candidate) => candidate.name === name);
  if (!project) throw new Error(`The project ${name} was not created.`);
  return project;
}

/** The scored snippets of a project, as the API sees them. */
async function scoredSnippets(core: string, projectId: string) {
  const response = await fetch(
    `${core}/v1/projects/${encodeURIComponent(projectId)}/snippets/scored`
  );
  if (!response.ok) {
    throw new Error(`Could not list the scored snippets: ${response.status}`);
  }
  return (await response.json()) as ScoredSnippet[];
}

/** The id of the first snippet the core holds. */
async function firstSnippetId(core: string): Promise<string> {
  const snippets = (await (await fetch(`${core}/v1/snippets?limit=50&offset=0`)).json()) as {
    id: string;
  }[];
  if (snippets.length === 0) {
    throw new Error("The core holds no snippets, so the fixture import failed.");
  }
  return snippets[0].id;
}

/** Create a project through the UI and return its id. */
async function createProject(page: import("@playwright/test").Page, name: string) {
  const brief = await deriveBrief();

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Brief").fill(brief);
  await page.getByRole("button", { name: "Create project" }).click();

  await expect(page.getByRole("button", { name })).toBeVisible();
}

test("a low-scoring snippet can be probed, and the answer floors it", async ({
  page,
}) => {
  const projectName = `Probe E2E ${Date.now()}`;
  const core = await fetchCore();

  await createProject(page, projectName);

  await page.getByRole("button", { name: projectName }).click();
  await page.getByRole("button", { name: "Open Cart" }).click();

  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();

  // Find a snippet the scorer thinks is worth nothing. The bands are
  // relative to the list, so the band is read from the badge rather than
  // guessed from a fixed score.
  const badges = page.getByTestId("score-badge");
  await expect(badges.first()).toBeVisible();
  const bands = await badges.evaluateAll((nodes) =>
    nodes.map((node) => (node as HTMLElement).dataset.band)
  );
  const lowIndex = bands.findIndex((band) => band === "low");
  expect(lowIndex).toBeGreaterThan(-1);

  const checkboxes = page.locator('.kfx-snippet-list input[type="checkbox"]');
  await expect(checkboxes.nth(lowIndex)).not.toBeChecked();

  // The id of the snippet, so the API can be asked about it afterwards.
  const checkboxId = await checkboxes.nth(lowIndex).getAttribute("id");
  const snippetId = (checkboxId ?? "").replace(/^snippet-/, "");
  expect(snippetId).not.toBe("");

  // Ticking it asks why, rather than taking the scorer's word for it.
  await checkboxes.nth(lowIndex).click();

  const modal = page.getByRole("dialog");
  await expect(modal).toBeVisible();
  await expect(modal).toContainText("Why does this matter?");
  await expect(checkboxes.nth(lowIndex)).not.toBeChecked();

  // A probe with no answer is not a probe, so the button waits for one.
  const submit = modal.getByRole("button", { name: "Submit probe" });
  await expect(submit).toBeDisabled();

  await modal.getByLabel("Intent", { exact: true }).fill("records the storage decision");
  await modal.getByLabel("Impact", { exact: true }).fill("rules out the flat-file store");
  await modal
    .getByLabel("Rationale", { exact: true })
    .fill("it is the decision the project is about");
  await modal.getByLabel("Confidence", { exact: true }).fill("settled");

  await expect(submit).toBeEnabled();
  await submit.click();
  await expect(modal).toHaveCount(0);

  // The answer floored the snippet. The list is sorted by score, so the
  // floored snippet is the first one now.
  await expect(page.getByTestId("score-badge").first()).toContainText("0.90");

  // And it is in the cart, because a snippet the user says matters is
  // there whether the scorer agrees or not.
  const cartItems = page.locator(".kfx-cart-sidebar .kfx-list-item");
  await expect(cartItems).toHaveCount(1);

  // The server agrees. A settled probe puts the snippet at 0.90 or above,
  // and reports the floor beside the signals rather than folding it in.
  const project = await findProject(core, projectName);
  const scored = await scoredSnippets(core, project.id);
  const probed = scored.find((snippet) => snippet.id === snippetId);
  expect(probed, "the probed snippet should still be scored").toBeDefined();
  expect(probed!.score).toBeGreaterThanOrEqual(0.9);
  expect(probed!.score_details.probe_floor).toBe(0.9);
});

test("a probe waiting for an answer can be answered from the queue", async ({
  page,
}) => {
  const projectName = `Probe Queue E2E ${Date.now()}`;
  const core = await fetchCore();

  await createProject(page, projectName);

  const project = await findProject(core, projectName);
  const snippetId = await firstSnippetId(core);

  // A probe raised elsewhere and left unanswered. The queue exists for the
  // probes the user has not got to yet, and a client other than this one
  // can raise them -- so the setup goes through the API, the same way the
  // extension or the CLI would.
  const raised = await fetch(`${core}/v1/probes`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      snippet_id: snippetId,
      project_id: project.id,
      trigger: "user_force_check",
      questions: ["What is this?"],
    }),
  });
  expect(raised.status).toBe(201);
  const probeId = ((await raised.json()) as { id: string }).id;

  // The queue is fetched when the snippet browser opens, so the probe has
  // to be raised before it is.
  await page.getByRole("button", { name: projectName }).click();
  await page.getByRole("button", { name: "Open Cart" }).click();
  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();

  const queue = page.getByTestId("probe-queue");
  await expect(queue).toBeVisible();
  await expect(queue).toContainText("1 probe waiting for an answer");

  // Answering it settles it and puts the floor under the snippet.
  await queue.getByRole("button", { name: "Answer" }).click();

  const modal = page.getByRole("dialog");
  await expect(modal).toBeVisible();
  await expect(modal).toContainText("Answer this probe");

  await modal.getByLabel("Intent", { exact: true }).fill("records the storage decision");
  await modal.getByLabel("Rationale", { exact: true }).fill("it is what the project is about");
  await modal.getByRole("button", { name: "Submit answer" }).click();
  await expect(modal).toHaveCount(0);

  // It is no longer waiting.
  await expect(page.getByTestId("probe-queue")).toHaveCount(0);

  // And the server says it is settled, with the floor that means.
  const stillPending = (await (
    await fetch(`${core}/v1/probes/pending?project_id=${project.id}`)
  ).json()) as { id: string }[];
  expect(
    stillPending.map((probe) => probe.id),
    "a settled probe is no longer pending"
  ).not.toContain(probeId);

  const scored = await scoredSnippets(core, project.id);
  const probed = scored.find((snippet) => snippet.id === snippetId);
  expect(probed, "the probed snippet should still be scored").toBeDefined();
  expect(probed!.score).toBeGreaterThanOrEqual(0.9);
  expect(probed!.score_details.probe_floor).toBe(0.9);
});
