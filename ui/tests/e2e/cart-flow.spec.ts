import { expect, test } from "@playwright/test";

/**
 * The cart round trip, driven the way a person does it.
 *
 * Create a project, open its cart view, tick a snippet, watch it appear in the
 * sidebar, then remove it and watch it go. Every layer beneath — fetch, CORS,
 * Axum, the cart service, SQLite — already has unit or integration tests; what
 * none of them prove is that a browser driving the real server ends up with the
 * right thing on screen.
 *
 * The snippets come from the fixture import in `playwright.config.ts`. If that
 * import ever stops working, this test fails on the "there are snippets" step
 * rather than silently asserting nothing, which is what makes it a test.
 */
test("adds a snippet to the cart and removes it", async ({ page }) => {
  const projectName = `Cart E2E ${Date.now()}`;

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  // A project to hold the cart.
  await page.getByRole("button", { name: "New project" }).click();
  await page.getByLabel("Name").fill(projectName);
  await page.getByLabel("Brief").fill("Cart flow");
  await page.getByRole("button", { name: "Create project" }).click();

  await expect(page.getByRole("button", { name: projectName })).toBeVisible();
  await page.getByRole("button", { name: projectName }).click();

  // Into the split view.
  await page.getByRole("button", { name: "Open Cart" }).click();
  await expect(page.getByRole("heading", { name: "Cart" })).toBeVisible();
  await expect(page.getByText("Cart is empty.")).toBeVisible();

  // The fixture import must have produced snippets. Asserting this instead of
  // skipping is the difference between a test and a formality.
  await expect(page.getByRole("heading", { name: "Snippets" })).toBeVisible();
  const checkboxes = page.locator('input[type="checkbox"]');
  await expect(checkboxes.first()).toBeVisible();
  expect(await checkboxes.count()).toBeGreaterThan(0);

  // Tick the first snippet. The checkbox is the control; its label is the text.
  const first = checkboxes.first();
  await expect(first).not.toBeChecked();
  // `check()` insists the box flips synchronously, but this is a controlled
  // input: the tick only arrives after the add round-trips and the cart is
  // refetched. Click, then wait for the state.
  await first.click();
  await expect(first).toBeChecked();

  // The sidebar is a separate component with its own fetch, so this proves the
  // cart mutation reached the server and came back.
  const cartItems = page.locator(".kfx-cart-sidebar .kfx-list-item");
  await expect(cartItems).toHaveCount(1);
  await expect(page.getByText("Cart is empty.")).toHaveCount(0);
  await expect(first).toBeChecked();

  // Remove it again.
  await page.getByRole("button", { name: "Remove from cart" }).click();

  await expect(cartItems).toHaveCount(0);
  await expect(page.getByText("Cart is empty.")).toBeVisible();
  await expect(first).not.toBeChecked();
});
