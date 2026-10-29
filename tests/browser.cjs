// Optional browser regression check. Playwright is a development dependency only.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || "playwright");
const fs = require("node:fs");
const path = require("node:path");
(async () => {
  const root = path.resolve(__dirname, "..");
  const base = process.env.GATEWAY_URL || "http://127.0.0.1:8787";
  const env = fs.readFileSync(path.join(root, ".env"), "utf8");
  const password =
    process.env.ADMIN_PASSWORD ||
    JSON.parse(
      env
        .split("\n")
        .find((s) => s.startsWith("ADMIN_PASSWORD="))
        .slice(15),
    );
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CHROMIUM_PATH
      ? { executablePath: process.env.CHROMIUM_PATH }
      : {}),
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1040 },
  });
  const keyName = "Browser verification " + Date.now();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("dialog", (d) => d.accept());
  const shots = path.join(root, "data/screenshots");
  fs.mkdirSync(shots, { recursive: true });
  await page.goto(base + "/admin");
  await page.getByRole("heading", { name: "Welcome back" }).waitFor();
  await page.screenshot({
    path: path.join(shots, "login.png"),
    fullPage: true,
  });
  await page.locator("#password").fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.getByRole("heading", { name: "Gateway overview" }).waitFor();
  await page.screenshot({
    path: path.join(shots, "overview.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "API keys", exact: true }).click();
  await page.getByRole("button", { name: "Create key", exact: true }).click();
  await page.locator("#key-name").fill(keyName);
  await page.locator("#key-rpm").fill("15");
  await page.locator("#key-models").selectOption("opencode-go/beam-free");
  await page
    .locator("#key-form")
    .getByRole("button", { name: "Create key", exact: true })
    .click();
  await page.getByRole("heading", { name: "Your API key is ready" }).waitFor();
  if (!(await page.locator(".secret").textContent()).startsWith("sk-gr-"))
    throw new Error("Missing one-time key");
  await page.getByRole("button", { name: "Done", exact: true }).click();
  const row = page.getByRole("row").filter({ hasText: keyName });
  await row.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator("#key-rpm").fill("10");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await page
    .getByRole("row")
    .filter({ hasText: keyName })
    .getByText("10 / min", { exact: true })
    .waitFor();
  await page
    .getByRole("row")
    .filter({ hasText: keyName })
    .getByRole("button", { name: "Revoke", exact: true })
    .click();
  await page
    .getByRole("row")
    .filter({ hasText: keyName })
    .getByText("Revoked", { exact: true })
    .waitFor();
  await page.getByRole("button", { name: "Models", exact: true }).click();
  await page.locator("#model-search").fill("deepseek");
  if ((await page.locator(".model-card").count()) !== 2)
    throw new Error("Model search failed");
  await page.getByRole("button", { name: "Request logs", exact: true }).click();
  await page.locator("[data-log]").first().click();
  await page.getByRole("heading", { name: "Request details" }).waitFor();
  await page.getByRole("button", { name: "Close dialog" }).click();
  await page
    .getByRole("button", { name: "Setup & settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Admin activity", exact: true })
    .click();
  await page.getByRole("heading", { name: "Admin activity" }).waitFor();
  await page.getByRole("button", { name: "Close dialog" }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Overview", exact: true }).click();
  await page.getByRole("heading", { name: "Gateway overview" }).waitFor();
  await page.screenshot({
    path: path.join(shots, "mobile.png"),
    fullPage: true,
  });
  const width = await page.evaluate(() => ({
    body: document.body.scrollWidth,
    screen: innerWidth,
  }));
  if (width.body > width.screen + 1)
    throw new Error("Mobile page overflows viewport: " + JSON.stringify(width));
  await page
    .locator(".topbar")
    .getByRole("button", { name: "Sign out" })
    .click();
  await page.getByRole("heading", { name: "Welcome back" }).waitFor();
  if (errors.length)
    throw new Error("Browser errors: " + JSON.stringify(errors));
  await browser.close();
  console.log(
    "Browser checks passed: login, key create/edit/revoke, model search, log details, admin audit, mobile layout, logout.",
  );
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
