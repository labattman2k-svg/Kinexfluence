import { expect, test, type Locator, type Page } from "@playwright/test";

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
 * The ports the core might be on, in the order to try them: the core binds
 * the first free port, and `reuseExistingServer` means an already-running
 * core on any port could be answering.
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

/** A snippet as the scored-snippets endpoint renders it. */
type ScoredSnippet = { id: string; score: number; score_details: { probe_floor: number | null } };

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

/** Create a project through the UI. */
async function createProject(page: Page, name: string) {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();

  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(name);
  // The brief names nothing in the fixture, so the scorer has no reason to
  // like any snippet and the low band -- the one this flow probes -- is
  // populated. The other E2E test derives its brief from the fixture
  // instead, because it needs the high band to be populated.
  await page.getByLabel("Brief").fill("the storage decision");
  await page.getByRole("button", { name: "Create project" }).click();

  await expect(page.getByRole("button", { name })).toBeVisible();
}

/** Open the snippet browser of a project. */
async function openSnippets(page: Page, name: string) {
  await page.getByRole("button", { name }).click();
  await page.getByRole("button", { name: "Open Cart" }).click();
  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();
}

/** Answer the four questions in the open modal. */
async function answerProbe(modal: Locator) {
  await modal.getByLabel("Intent", { exact: true }).fill("records the storage decision");
  await modal.getByLabel("Impact", { exact: true }).fill("rules out the flat-file store");
  await modal.getByLabel("Rationale", { exact: true }).fill("it is what the project is about");
  await modal.getByLabel("Confidence", { exact: true }).fill("settled");
}

/**
 * The server's own account of a probed snippet: a settled probe puts the
 * snippet at 0.90 or above, and reports the floor beside the signals.
 */
async function expectFloored(core: string, projectId: string, snippetId: string) {
  const response = await fetch(
    `${core}/v1/projects/${encodeURIComponent(projectId)}/snippets/scored`
  );
  if (!response.ok) {
    throw new Error(`Could not list the scored snippets: ${response.status}`);
  }
  const scored = (await response.json()) as ScoredSnippet[];
  const probed = scored.find((snippet) => snippet.id === snippetId);
  expect(probed, "the probed snippet should still be scored").toBeDefined();
  expect(probed!.score).toBeGreaterThanOrEqual(0.9);
  expect(probed!.score_details.probe_floor).toBe(0.9);
}

test("a low-scoring snippet can be probed, and the answer floors it", async ({
  page,
}) => {
  const projectName = `Probe E2E ${Date.now()}`;
  const core = await fetchCore();

  await createProject(page, projectName);
  await openSnippets(page, projectName);

  // Find a snippet the scorer thinks is worth nothing. The bands are
  // relative to the list, so the band is read from the badge rather than
  // guessed from a fixed score.
  const badges = page.getByTestId("score-badge");
  await expect(badges.first()).toBeVisible();
  const bands = await badges.evaluateAll((nodes) =>
    nodes.map((node) => (node as HTMLElement).dataset.band)
  );
  const lowIndex = bands.findIndex((band) => band === "low");
  expect(lowIndex, "the fixture should hold a snippet the scorer dismisses").toBeGreaterThan(-1);

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

  await answerProbe(modal);
  await expect(submit).toBeEnabled();
  await submit.click();
  await expect(modal).toHaveCount(0);

  // The answer floored the snippet. The list is sorted by score, so the
  // floored snippet is the first one now.
  await expect(page.getByTestId("score-badge").first()).toContainText("0.90");

  // And it is in the cart, because a snippet the user says matters is
  // there whether the scorer agrees or not.
  await expect(page.locator(".kfx-cart-sidebar .kfx-list-item")).toHaveCount(1);

  // And the server agrees.
  const project = await findProject(core, projectName);
  await expectFloored(core, project.id, snippetId);
});

test("a probe waiting for an answer can be answered from the queue", async ({
  page,
}) => {
  const projectName = `Probe Queue E2E ${Date.now()}`;
  const core = await fetchCore();

  await createProject(page, projectName);

  const project = await findProject(core, projectName);
  const listed = (await (await fetch(`${core}/v1/snippets?limit=1&offset=0`)).json()) as {
    id: string;
  }[];
  expect(listed.length, "the fixture import should have produced snippets").toBeGreaterThan(0);
  const snippetId = listed[0].id;

  // A probe raised elsewhere and left unanswered, the way the extension
  // would raise it. The queue is fetched when the snippet browser opens, so
  // the probe has to be raised before it is.
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

  await openSnippets(page, projectName);

  const queue = page.getByTestId("probe-queue");
  await expect(queue).toBeVisible();
  await expect(queue).toContainText("1 probe waiting for an answer");

  // Answering it settles it and puts the floor under the snippet.
  await queue.getByRole("button", { name: "Answer" }).click();

  const modal = page.getByRole("dialog");
  await expect(modal).toBeVisible();
  await expect(modal).toContainText("Answer this probe");

  await answerProbe(modal);
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

  await expectFloored(core, project.id, snippetId);
});
