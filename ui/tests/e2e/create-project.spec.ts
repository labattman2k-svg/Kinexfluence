import { expect, test } from "@playwright/test";

/**
 * The one end-to-end test.
 *
 * It uses the product the way a person does — click the button, type in the
 * fields, submit — and then asserts on what the next screen shows. Every layer
 * beneath it (fetch, CORS, Axum routing, the writer, SQLite) has unit or
 * integration tests; what none of them prove is that a real browser driving a
 * real server ends up with the project on screen.
 */
test("creates a project and shows it in the list", async ({ page }) => {
  // Unique per run, so a database left over from an earlier run cannot make
  // the final assertion pass on someone else's row.
  const name = `E2E Project ${Date.now()}`;

  await page.goto("/");

  // The list has to be up before anything else, which also means the port
  // probe found the core: without it the panel renders an error instead.
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);

  await page.getByRole("button", { name: "New project" }).click();
  await expect(page.getByRole("heading", { name: "New project" })).toBeVisible();

  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Brief").fill("Created by Playwright");

  await page.getByRole("button", { name: "Create project" }).click();

  // Back on the list, which has refetched, and the new project is in it.
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByRole("button", { name })).toBeVisible();
});
