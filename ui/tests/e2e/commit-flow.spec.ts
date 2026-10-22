import { expect, test, type Page } from "@playwright/test";

/**
 * The commit flow, driven the way a person does it.
 *
 * The cart is a list of candidates; the context pack is built from the
 * committed ones. This test closes the loop: it fills the cart, commits it,
 * and checks that the pack now holds what was in the cart.
 *
 * The pack is checked through the API rather than through the screen, because
 * the pack is what the user takes away, and a button that says "committed"
 * while the pack is empty would pass a visual test and fail the user.
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

/** A snippet as the scored-snippets endpoint renders it. */
type ScoredSnippet = { id: string; text: string };

test("commits the cart, and the export holds what was in it", async ({ page }) => {
  const projectName = `Commit E2E ${Date.now()}`;
  const core = await fetchCore();

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();

  // A project to commit into.
  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(projectName);
  await page.getByLabel("Brief").fill("Cart flow");
  await page.getByRole("button", { name: "Create project" }).click();
  await expect(page.getByRole("button", { name: projectName })).toBeVisible();

  await page.getByRole("button", { name: projectName }).click();
  await page.getByRole("button", { name: "Open Cart" }).click();
  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();

  // The high-scoring snippets start checked, so the selection is put into a
  // known state first: exactly one snippet, chosen here, and nothing else.
  // A snippet in the low band is the one the UI asks about before it accepts
  // it, and this test is about the commit.
  await expect(page.getByTestId("score-badge").first()).toBeVisible();
  const bands = await page.getByTestId("score-badge").evaluateAll((nodes) =>
    nodes.map((node) => (node as HTMLElement).dataset.band)
  );
  const wanted = bands.findIndex((band) => band !== "low");
  expect(wanted, "the fixture should hold a snippet the scorer likes").toBeGreaterThan(-1);

  const checkboxes = page.locator('.kfx-snippet-list input[type="checkbox"]');
  for (const checkbox of await checkboxes.all()) {
    if (await checkbox.isChecked()) {
      await checkbox.uncheck();
    }
  }
  const target = checkboxes.nth(wanted);
  await target.click();
  await expect(target).toBeChecked();

  // The id of the snippet, so the export can be checked for its text.
  const checkboxId = (await target.getAttribute("id")) ?? "";
  const snippetId = checkboxId.replace(/^snippet-/, "");
  expect(snippetId).not.toBe("");

  await page.getByRole("button", { name: /Add \d+ to Cart/ }).click();
  await expect(page.locator(".kfx-cart-sidebar .kfx-list-item")).toHaveCount(1);

  // The commit. The button is disabled while the cart is empty, so its
  // becoming enabled is itself part of what is being tested.
  const commit = page.getByRole("button", { name: "Commit to Project" });
  await expect(commit).toBeEnabled();
  await commit.click();

  // The sidebar says what the server did, and the cart is empty.
  await expect(page.getByRole("status")).toContainText("Committed 1 item");
  await expect(page.locator(".kfx-cart-sidebar .kfx-list-item")).toHaveCount(0);
  await expect(page.getByText("Cart is empty.")).toBeVisible();

  // And the pack holds the snippet that was in the cart.
  const project = await findProject(core, projectName);
  const scored = (await (
    await fetch(`${core}/v1/projects/${project.id}/snippets/scored`)
  ).json()) as ScoredSnippet[];
  const snippet = scored.find((candidate) => candidate.id === snippetId);
  expect(snippet, "the committed snippet should still be scored").toBeDefined();

  const exported = await fetch(`${core}/v1/projects/${project.id}/export`);
  expect(exported.status).toBe(200);
  const markdown = await exported.text();
  expect(markdown).not.toContain("No items have been committed");
  // The first line is what the pack quotes; a fragment of several lines is
  // indented in the pack, so the whole text is not a substring of it.
  expect(markdown).toContain(snippet!.text.split("\n")[0]);
});
