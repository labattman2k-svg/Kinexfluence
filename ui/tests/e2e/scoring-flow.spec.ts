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
 * The brief the project is created with.
 *
 * "Python" is an entity the fixture's snippets actually mention, so the
 * scores spread across the three bands instead of piling up in one. That
 * matters: a list where every snippet lands in the same band would pass the
 * auto-check assertions without ever exercising the thresholds.
 */
const BRIEF = "Python";

test("shows scores, explains them, and bulk-adds the auto-checked snippets", async ({
  page,
}) => {
  const projectName = `Scoring E2E ${Date.now()}`;

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  // A project to score against.
  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(projectName);
  await page.getByLabel("Brief").fill(BRIEF);
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
  const bands = await badges.evaluateAll((nodes) =>
    nodes.map((node) => (node as HTMLElement).dataset.band)
  );
  expect(new Set(bands).size).toBeGreaterThan(1);

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
