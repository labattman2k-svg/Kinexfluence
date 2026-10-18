import { defineConfig } from "@playwright/test";

/**
 * End-to-end configuration.
 *
 * The app under test is the React UI served by Vite, talking to a real core
 * over HTTP. Both are started here, so the test exercises the same stack a user
 * runs: browser -> fetch -> CORS -> Axum -> SQLite.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  // A browser test that needs a Rust server and a Vite server is not fast. The
  // default 30s would time out on a cold `cargo run` alone.
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://localhost:5173",
    trace: "retain-on-failure",
  },
  webServer: [
    {
      // A real core on a real socket, with its own database under `target/`,
      // which is already ignored by git.
      command:
        "cargo run --quiet -p kfx-cli -- serve --port 47821 --db ./target/e2e.db",
      cwd: "..",
      // Wait for the health endpoint rather than the port: a listening socket
      // that is still migrating the database would answer late, and Playwright
      // only waits for the port by default.
      url: "http://127.0.0.1:47821/v1/health",
      reuseExistingServer: !process.env.CI,
      timeout: 240_000,
    },
    {
      command: "npm run dev",
      url: "http://localhost:5173",
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
  ],
});
