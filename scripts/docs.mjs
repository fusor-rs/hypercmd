import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { checkBrowser } from "./browser-app.mjs";

const root = resolve(import.meta.dirname, "..");
const navigation = JSON.parse(await readFile(resolve(root, "apps/docs/navigation.json"), "utf8"));

await checkBrowser("apps/docs", async (page, origin) => {
  const docs = origin + "docs/";
  for (const { slug } of navigation) {
    const source = `${slug || "index"}.md`;
    const markdown = await readFile(resolve(root, "docs", source), "utf8");
    const title = markdown.split("\n")[0].slice(2);
    await page.goto(docs + slug);
    await page.getByRole("heading", { name: title, exact: true }).waitFor();
    assert.equal(await page.title(), `${title} · hypercmd`);
    await checkTypography(page, slug || "index");
    const response = await page.request.get(docs + "content/" + source);
    assert.equal(response.status(), 200);
    assert.equal(await response.text(), markdown);
    await page.setViewportSize({ width: 390, height: 844 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), slug);
    assert(await page.locator(".article td:first-child code").evaluateAll(nodes =>
      nodes.every(node => node.getClientRects().length === 1)), slug + " keeps table names intact");
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(docs);
  await page.getByRole("heading", { name: "Build terminal apps with HTML and Rust" }).waitFor();
  const profile = page.locator(".sections").getByRole("link", { name: "HTML and CSS reference" });
  assert.equal(await profile.getAttribute("href"), "/docs/profile");
  assert.equal(await page.getByRole("link", { name: "installation and first application walkthrough" })
    .getAttribute("href"), "https://github.com/fusor-rs/hypercmd/blob/main/README.md#get-started");
  await profile.click();
  await page.getByRole("heading", { name: "HTML and CSS reference", exact: true }).waitFor();
  await page.goBack();
  await page.getByRole("heading", { name: "Build terminal apps with HTML and Rust" }).waitFor();
  await page.getByRole("searchbox").fill("no-matching-topic");
  await page.locator(".search-empty").waitFor();
  await page.getByRole("searchbox").press("Escape");
  await page.getByRole("button", { name: "Toggle color theme" }).click();
  await page.reload();
  await page.locator(".site.dark").waitFor();
  await page.screenshot({ path: resolve(root, "apps/docs/target/docs-desktop.png"), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Toggle navigation" }).click();
  await page.locator(".sidebar").getByRole("link", { name: "Introduction" }).click();
  await page.locator(".sidebar").waitFor({ state: "hidden" });
  await page.screenshot({ path: resolve(root, "apps/docs/target/docs-mobile.png"), fullPage: true });
  await page.goto(docs + "missing");
  await page.getByRole("heading", { name: "Page not found" }).waitFor();
});
console.log(`PASS: ${navigation.length} Hypercmd guides, Markdown downloads, navigation and mobile layout`);

async function checkTypography(page, slug) {
  const paragraphs = await page.locator(".article .markdown p").evaluateAll(nodes =>
    nodes.map(node => {
      const style = getComputedStyle(node);
      return { size: style.fontSize, family: style.fontFamily };
    }));
  assert(paragraphs.length > 1, slug + " has an introduction and body text");
  assert(
    paragraphs.slice(1).every(paragraph =>
      paragraph.size === "14px" && paragraph.family === paragraphs[0].family),
    slug + " uses the standard body typography after its introduction",
  );
}
