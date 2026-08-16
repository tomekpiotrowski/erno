import { expect, test } from "@playwright/test";

const api = process.env.API_URL ?? "http://127.0.0.1:3001";

test("API liveness", async ({ request }) => {
  const response = await request.get(`${api}/liveness`);
  expect(response.ok()).toBeTruthy();
});
