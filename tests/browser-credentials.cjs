// Browser onboarding checks with an isolated database and fake keys; no Go calls.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || "playwright");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const net = require("node:net");
const { spawn } = require("node:child_process");

(async () => {
  const root = path.resolve(__dirname, "..");
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "go-router-browser-"));
  const listener = net.createServer();
  await new Promise((resolve) => listener.listen(0, "127.0.0.1", resolve));
  const port = listener.address().port;
  await new Promise((resolve) => listener.close(resolve));
  const base = `http://127.0.0.1:${port}`;
  const password = "browser-test-only-admin-password";
  const key = "browser-only-fake-upstream-key-123456";
  const server = spawn(process.env.GO_ROUTER_BIN || path.join(root, "target/debug/go-router"), [], {
    cwd: temporary,
    env: { ...process.env, BIND: `127.0.0.1:${port}`, DATABASE_PATH: path.join(temporary, "test.db"),
      MODEL_CATALOG_PATH: path.join(root, "config/models.json"), ADMIN_PASSWORD: password,
      OPENCODE_GO_API_KEY: "", GO_BASE_URL: "http://127.0.0.1:9", PUBLIC_URL: base, COOKIE_SECURE: "false" },
    stdio: "ignore",
  });
  const exited = new Promise((resolve) => server.once("exit", resolve));
  let browser;
  try {
    let ready = false;
    for (let i = 0; i < 150; i++) {
      try { if ((await fetch(base + "/readyz")).ok) { ready = true; break; } } catch {}
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
    if (!ready) throw new Error("Test gateway failed to start");
    browser = await chromium.launch({ headless: true,
      ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
    const page = await browser.newPage({ viewport: { width: 1440, height: 1040 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("dialog", (dialog) => dialog.accept());
    await page.goto(base + "/admin");
    await page.locator("#password").fill(password);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await page.getByRole("button", { name: "Add your Go API key", exact: true }).click();
    const input = page.locator("#upstream-key");
    if ((await input.getAttribute("type")) !== "password") throw new Error("Key input is not masked");
    await input.fill(key);
    await page.getByRole("button", { name: "Save Go key", exact: true }).click();
    await page.getByRole("button", { name: "Replace Go key", exact: true }).waitFor();
    if (await input.inputValue()) throw new Error("Saved key was retained in input");
    const settings = await page.evaluate(() => fetch("/admin/api/settings").then((r) => r.json()));
    if (!settings.upstream_configured || JSON.stringify(settings).includes(key)) throw new Error("Invalid or leaking credential status");
    if ((await page.content()).includes(key)) throw new Error("Key exposed in page markup");
    await input.fill(key + "-rotated");
    await page.getByRole("button", { name: "Replace Go key", exact: true }).click();
    await page.getByText("Go key saved. New requests can use it immediately.", { exact: true }).waitFor();
    await page.reload();
    await page.getByRole("button", { name: "Setup & settings", exact: true }).click();
    await page.getByRole("button", { name: "Remove Go key", exact: true }).click();
    await page.getByRole("button", { name: "Save Go key", exact: true }).waitFor();
    await page.locator("#upstream-status").filter({ hasText: "Go key required" }).waitFor();
    const screenshots = path.join(root, "data/screenshots");
    fs.mkdirSync(screenshots, { recursive: true });
    await page.screenshot({ path: path.join(screenshots, "credential-setup.png"), fullPage: true });
    await page.setViewportSize({ width: 390, height: 844 });
    const width = await page.evaluate(() => ({ body: document.body.scrollWidth, screen: innerWidth }));
    if (width.body > width.screen + 1) throw new Error("Credential form overflows mobile viewport");
    await page.screenshot({ path: path.join(screenshots, "credential-setup-mobile.png"), fullPage: true });
    if (errors.length) throw new Error(errors.join("; "));
    console.log("Browser checks passed: keyless onboarding, masked save, rotation, removal, no key disclosure, mobile layout.");
  } finally {
    if (browser) await browser.close();
    server.kill("SIGTERM");
    await exited;
    fs.rmSync(temporary, { recursive: true, force: true });
  }
})().catch((error) => { console.error(error.message); process.exit(1); });
