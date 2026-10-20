import { expect, test } from "@playwright/test";

/**
 * The scoring flow, driven the way a person does it.
 *
 * Open a project's snippet browser, read a score, ask why it scored that way,
 * notice that the strong snippets are already ticked, overrule that default,
 * and commit the ticks to the cart. The scores and their bands come from the
 * real scorer in the core, over the real scored-snippets endpoint, so this
 * test fails if the API contract drifts rather than only if the rendering
 * does.
 *
 * The fixture import in `playwright.config.ts` supplies the snippets. If that
 * import ever stops working, this test fails on the "there are snippets" step
 * rather than silently asserting nothing, which is what makes it a test.
 */

/**
 * The port the core is on, discovered the way the UI discovers it.
 *
 * The core binds the first free port from its configured starting point, so
 * the UI cannot assume one. The Playwright config asks for 47899, but the
 * core moves on if that port is taken -- and `reuseExistingServer` means an
 * already-running core of any port could be answering instead. So the test
 * probes the same range the UI does, and takes whichever answers.
 */
const CANDIDATE_PORTS = [
  47899, 47821, 47822, 47823, 47824, 47825, 47826, 47827, 47828, 47829, 47830,
];

/** The port that answered, once one has. */
let cachedCoreUrl: string | null = null;

/** Find the core, and the snippets it holds. */
async function fetchCore(): Promise<string> {
  if (cachedCoreUrl) return cachedCoreUrl;

  for (const port of CANDIDATE_PORTS) {
    const url = `http://127.0.0.1:${port}`;
    try {
      const response = await fetch(`${url}/v1/health`);
      if (!response.ok) continue;
      const body = (await response.json()) as { status?: string };
      if (body.status === "ok") {
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
 * A brief guaranteed to match the snippets, whatever the fixture holds.
 *
 * The project brief is what the scorer extracts its entities from, and an
 * entity only scores if it appears verbatim in a snippet. A hardcoded brief
 * like "Python" therefore depends on the fixture happening to contain that
 * word -- true today, false the moment the fixture changes.
 *
 * So the brief is derived from the snippets themselves: every snippet is
 * searched for a token that the scorer would extract as an entity, and the
 * longest one wins. The rules below are the scorer's own -- snake_case,
 * camelCase, and Capitalised words of more than three characters -- so the
 * token chosen here is one the scorer will extract, and the entity match
 * follows.
 *
 * The token is searched across every snippet rather than just the first,
 * because a snippet need not contain any identifier at all.
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

  // The scorer's own rules, so the token chosen here is one the scorer
  // will extract as an entity.
  const isEntity = (token: string): boolean => {
    if (token.length <= 3 || !/^[A-Za-z0-9_]+$/.test(token)) return false;

    const isSnake =
      token.includes("_") &&
      !token.startsWith("_") &&
      !token.endsWith("_") &&
      /[A-Za-z]/.test(token);
    const isCamel = /^[a-z]/.test(token) && /[A-Z]/.test(token);
    const isCapitalized = /^[A-Z]/.test(token);

    return isSnake || isCamel || isCapitalized;
  };

  const entities = snippets
    .flatMap((snippet) => snippet.text.split(/\s+/))
    .map((token) => token.replace(/^[^A-Za-z0-9_]+|[^A-Za-z0-9_]+$/g, ""))
    .filter(isEntity)
    .sort((a, b) => b.length - a.length);

  if (entities.length > 0) {
    return entities[0];
  }

  // The fixture holds no identifier at all. Fall back to a word from the
  // first snippet: the entity match will be empty, but the badges, the
  // bands, and the auto-check are all still exercised.
  return snippets[0].text.split(/\s+/).find((word) => word.length > 3) ?? "brief";
}

test("shows scores, explains them, and bulk-adds the auto-checked snippets", async ({
  page,
}) => {
  const projectName = `Scoring E2E ${Date.now()}`;

  // The brief is derived from the fixture rather than hardcoded, so the
  // entity match -- and therefore the spread of scores across the three
  // bands -- holds whatever the fixture contains.
  const brief = await deriveBrief();

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  // A project to score against.
  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(projectName);
  await page.getByLabel("Brief").fill(brief);
  await page.getByRole("button", { name: "Create project" }).click();

  await expect(page.getByRole("button", { name: projectName })).toBeVisible();
  await page.getByRole("button", { name: projectName }).click();
  await page.getByRole("button", { name: "Open Cart" }).click();

  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();

  const badges = page.getByTestId("score-badge");
  await expect(badges.first()).toBeVisible();
  expect(await badges.count()).toBeGreaterThan(0);

  // Every score is shown to two decimals, and the bands are not all the same:
  // the thresholds have to be able to separate snippets.
  //
  // The middle band is the one that matters here. A snippet reaches it
  // because of the entity signal, and the entity only scores if the brief
  // contains a word the snippet also contains. A brief with no match leaves
  // every snippet either high or low, and the list would still look
  // plausible -- so asserting only that the bands differ would pass without
  // the brief matching anything. Asserting the middle band is present is
  // what proves the brief was derived correctly.
  const bands = await badges.evaluateAll((nodes) =>
    nodes.map((node) => (node as HTMLElement).dataset.band)
  );
  expect(bands).toContain("medium");
  expect(bands).toContain("low");

  const firstScore = await badges.first().innerText();
  expect(firstScore).toMatch(/^\s*[▲◆▼]\s*\d\.\d{2}/);

  // The strongest snippet is in the high band and starts checked. The score
  // is sorted descending by the server, so the first badge is the best one.
  expect(bands[0]).toBe("high");

  const checkboxes = page.locator('.kfx-snippet-list input[type="checkbox"]');
  await expect(checkboxes.first()).toBeChecked();

  // A weak snippet is not checked, so the auto-check is discriminating rather
  // than ticking everything.
  const lowIndex = bands.findIndex((band) => band === "low");
  expect(lowIndex).toBeGreaterThan(-1);
  await expect(checkboxes.nth(lowIndex)).not.toBeChecked();

  // The rationale is reachable: the info button reveals the breakdown.
  await page.locator(".kfx-score-info-btn").first().click();

  const panel = page.getByRole("tooltip").first();
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("Code");
  await expect(panel).toContainText("Markers");
  await expect(panel).toContainText("Entities");
  await expect(panel).toContainText("Position");

  // The breakdown carries the signals, and says which ones are still stubs.
  await expect(panel.locator(".kfx-score-row")).toHaveCount(4);
  await expect(panel).toContainText("stubbed");

  // The default is a proposal, and the user can overrule it. Unticking the
  // high snippet leaves nothing selected, because the weak ones were never
  // selected in the first place.
  await checkboxes.first().click();
  await expect(checkboxes.first()).not.toBeChecked();

  const addButton = page.getByRole("button", { name: /Add \d+ to Cart/ });
  await expect(addButton).toBeDisabled();
  await expect(addButton).toHaveText("Add 0 to Cart");
  await expect(page.getByText("Cart is empty.")).toBeVisible();

  // Tick it back. The button counts what it will actually change, which is
  // the ticked snippets that are not in the cart yet.
  await checkboxes.first().click();
  await expect(checkboxes.first()).toBeChecked();

  const selectedCount = await page
    .locator('.kfx-snippet-list input[type="checkbox"]:checked')
    .count();
  await expect(addButton).toBeEnabled();
  await expect(addButton).toHaveText(`Add ${selectedCount} to Cart`);

  // Commit the ticks. The sidebar is a separate component with its own
  // fetch, so the item appearing there proves the mutation reached the
  // server and came back.
  await addButton.click();

  const cartItems = page.locator(".kfx-cart-sidebar .kfx-list-item");
  await expect(cartItems).toHaveCount(selectedCount);
  await expect(page.getByText("Cart is empty.")).toHaveCount(0);

  // The cart items carry the score they were added with.
  await expect(page.getByTestId("cart-score-badge")).toHaveCount(selectedCount);

  // With everything added, there is nothing left to add.
  await expect(addButton).toBeDisabled();
  await expect(addButton).toHaveText("Add 0 to Cart");
});
