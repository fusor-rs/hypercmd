import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { chromium } from "playwright";

export async function checkBrowser(directory, verify, buildArguments = []) {
  const app = resolve(import.meta.dirname, "..", directory);
  const fusor = process.env.FUSOR_BIN ?? "fusor";
  execFileSync(fusor, [
    "build", ...buildArguments, "--manifest-path", join(app, "Cargo.toml"),
    "--debug", "--locked", "--offline",
  ], { cwd: app, stdio: "inherit", timeout: 180_000 });
  const { server, ready, origin } = await preview(fusor, app);
  let browser;
  try {
    await ready;
    browser = await chromium.launch();
    const page = await browser.newPage();
    page.setDefaultTimeout(15_000);
    const errors = [];
    page.on("console", message => {
      if (message.type() === "error") errors.push(message.text());
    });
    page.on("pageerror", error => errors.push(error.message));
    await verify(page, origin);
    assert.deepEqual(errors, []);
  } finally {
    await browser?.close();
    server.kill();
    if (server.exitCode === null) await new Promise(resolve => server.once("exit", resolve));
  }
}

async function preview(fusor, app) {
  const portPicker = createServer();
  await new Promise((resolve, reject) => {
    portPicker.once("error", reject);
    portPicker.listen(0, "127.0.0.1", resolve);
  });
  const port = portPicker.address().port;
  await new Promise(resolve => portPicker.close(resolve));
  const server = spawn(fusor, ["preview", join(app, "dist"), "--port", String(port)], { cwd: app });
  const ready = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("fusor preview did not become ready")), 10_000);
    server.once("error", error => { clearTimeout(timeout); reject(error); });
    server.once("exit", code => {
      clearTimeout(timeout);
      reject(new Error(`fusor preview exited: ${code}`));
    });
    server.stderr.on("data", bytes => {
      if (bytes.toString().includes("Ready.")) { clearTimeout(timeout); resolve(); }
    });
    server.stderr.pipe(process.stderr);
  });
  return { server, ready, origin: `http://127.0.0.1:${port}/` };
}
