import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { createServer } from "node:net";
import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "..");
const manifest = join(root, "tests/browser-consumer/Cargo.toml");
const app = dirname(manifest);
const fusor = process.env.FUSOR_BIN ?? "fusor";
execFileSync(fusor, ["build", "--manifest-path", manifest, "--debug", "--locked", "--offline"], {
  cwd: app, stdio: "inherit", timeout: 180_000,
});

const portPicker = createServer();
await new Promise((resolve, reject) => { portPicker.once("error", reject); portPicker.listen(0, "127.0.0.1", resolve); });
const port = portPicker.address().port;
await new Promise(resolve => portPicker.close(resolve));
const server = spawn(fusor, ["preview", join(app, "dist"), "--port", String(port)], { cwd: app });
const ready = new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("fusor preview did not become ready")), 10_000);
  server.once("error", error => { clearTimeout(timeout); reject(error); });
  server.once("exit", code => { clearTimeout(timeout); reject(new Error(`fusor preview exited: ${code}`)); });
  server.stderr.on("data", bytes => { if (bytes.toString().includes("Ready.")) { clearTimeout(timeout); resolve(); } });
  server.stderr.pipe(process.stderr);
});
let browser;
try {
  await ready;
  browser = await chromium.launch();
  const page = await browser.newPage();
  const errors = [];
  const fail = message => { errors.push(message); console.error(message); };
  page.on("console", message => { if (message.type() === "error") fail(message.text()); });
  page.on("pageerror", error => fail(error.message));
  const button = (parent, name) => parent.getByRole("button", { name, exact: true });
  await page.goto(`http://127.0.0.1:${port}/`);
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
  assert.deepEqual(errors, []);
  console.log(`PASS: shared component executes local state, keyed identity, callbacks, cancel and removal in Chromium ${browser.version()}; browser/terminal outputs coexist`);
} finally {
  await browser?.close();
  server.kill();
  if (server.exitCode === null) await new Promise(resolve => server.once("exit", resolve));
}
