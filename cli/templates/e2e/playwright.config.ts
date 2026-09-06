import { defineConfig } from "@playwright/test";

const api = process.env.API_URL ?? "http://127.0.0.1:3001";
const appUrl = process.env.APP_URL ?? "http://127.0.0.1:4200";
const appPort = new URL(appUrl).port || "4200";

export default defineConfig({
  testDir: ".",
  timeout: 30_000,
  use: {
    baseURL: appUrl,
  },
  // `erno test --e2e` starts the API on a free port and sets API_URL / APP_URL.
  webServer: process.env.SKIP_APP_SERVER
    ? undefined
    : {
        command: `bun run --cwd ../app start --port ${appPort} --host 127.0.0.1`,
        url: appUrl,
        reuseExistingServer: !process.env.ERNO_E2E && !process.env.CI,
        timeout: 180_000,
      },
  metadata: { api },
});
