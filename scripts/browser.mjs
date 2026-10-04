import assert from "node:assert/strict";
import { checkBrowser } from "./browser-app.mjs";

await checkBrowser("tests/browser-consumer", async (page, origin) => {
  const button = (parent, name) => parent.getByRole("button", { name, exact: true });
  await page.goto(origin);
  const rows = page.locator(".job");
  const first = rows.filter({ hasText: "Job 01" });
  const second = rows.filter({ hasText: "Job 02" });
  await button(first, "Inspect (0)").click();
  assert.equal(await page.locator("#notice").innerText(), "Inspecting job 1");
  await button(first, "Inspect (1)").waitFor();
  await button(second, "Inspect (0)").waitFor();
  const retained = await first.elementHandle();
  await button(page, "Reverse").click();
  assert.equal(await rows.last().evaluate((row, previous) => row === previous, retained), true);
  await button(first, "Inspect (1)").waitFor();
  await button(first, "Cancel work").click();
  assert.equal(await button(first, "Cancel work").isDisabled(), true);
  assert.equal(await button(first, "Inspect (1)").evaluate(button => getComputedStyle(button).backgroundColor), "rgb(23, 71, 130)");
  await button(first, "Remove").click();
  assert.equal(await retained.evaluate(row => row.isConnected), false);
  assert.equal(await rows.count(), 1);
});
console.log("PASS: shared component state, keyed identity, callbacks, cancellation and removal");
