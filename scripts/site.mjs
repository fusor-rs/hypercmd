import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { checkBrowser } from "./browser-app.mjs";

await checkBrowser("apps", async (page, origin) => {
  await page.goto(origin);
  await page.getByRole("heading", { name: "Write CLI applications using plain HTML" }).waitFor();
  assert.equal(
    await page.locator(".quick-start pre").innerText(),
    "hypercmd new my-app\ncd my-app\nhypercmd run",
  );
  await page.getByRole("link", { name: "CLI reference", exact: false }).click();
  await page.getByRole("heading", { name: "CLI reference", exact: true }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/docs/cli");
  await page.reload();
  await page.getByRole("heading", { name: "hypercmd new", exact: true }).waitFor();
  const markdown = await page.request.get(origin + "docs/content/cli.md");
  assert.equal(markdown.status(), 200);
  assert.equal(
    await markdown.text(),
    await readFile(new URL("../docs/cli.md", import.meta.url), "utf8"),
  );
  await page.getByRole("link", { name: "Home", exact: true }).click();
  await page.getByRole("heading", { name: "Write CLI applications using plain HTML" }).waitFor();
  await page.getByRole("link", { name: "Documentation", exact: false }).click();
  await page.getByRole("heading", { name: "Build terminal apps with HTML and Rust" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/docs/");
  await page.goto(origin + "docs/missing");
  await page.getByRole("heading", { name: "Page not found", exact: true }).waitFor();
}, ["--site"]);
console.log("PASS: quick start, landing/docs navigation, CLI deep links and Markdown source");
