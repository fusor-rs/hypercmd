import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";
import { checkBrowser } from "./browser-app.mjs";

const app = resolve(import.meta.dirname, "../apps/landing");
const examples = [
  ["Counter", "counter.html"],
  ["Filesystem", "search.html"],
  ["Tables", "tables.html"],
  ["Boxes", "boxes.html"],
  ["Menu", "menu.html"],
];

const interactions = {
  async Counter(page, terminal) {
    const increment = terminal.getByRole("button", { name: "+1", exact: true });
    await increment.click();
    await increment.focus();
    await page.keyboard.press("Enter");
    assert.match(await terminal.innerText(), /Count: 2\b/);
    await terminal.getByRole("button", { name: "Reset", exact: true }).click();
    assert.match(await terminal.innerText(), /Count: 0\b/);
  },
  async Filesystem(page, terminal) {
    const input = terminal.getByRole("textbox", { name: "Find files" });
    for (const [query, count] of [["counter", 2], ["not-a-project-file", 0], [".HTML", 5]]) {
      await input.fill(query);
      assert.match(await terminal.innerText(), new RegExp(`\\b${count} matches`));
      assert.equal(await input.inputValue(), query);
      assert(await input.evaluate(node => node === document.activeElement));
    }
    await input.fill("counter");
    assert.match(await terminal.innerText(), /src\/examples\/counter.rs/);
    assert.match(await terminal.innerText(), /ui\/counter.html/);
  },
  async Tables(page, terminal) {
    const files = await Promise.all(examples.map(async ([, filename]) => ({
      path: `ui/${filename}`, bytes: (await stat(resolve(app, "ui", filename))).size,
    })));
    const paths = () => terminal.innerText().then(text => text.match(/ui\/[a-z]+\.html/g));
    assert.deepEqual(await paths(), files.map(file => file.path).sort());
    await terminal.getByRole("button", { name: "Sort by size" }).click();
    files.sort((left, right) => right.bytes - left.bytes || left.path.localeCompare(right.path));
    assert.deepEqual(await paths(), files.map(file => file.path));
    for (const file of files) assert((await terminal.innerText()).includes(String(file.bytes)));
  },
  async Boxes(page, terminal) {
    const before = await terminal.innerText();
    assert(before.split("\n").some(line => line.includes("HTML") && line.includes("Rust")));
    await terminal.getByRole("button", { name: "Toggle layout" }).click();
    const after = await terminal.innerText();
    assert(!after.split("\n").some(line => line.includes("HTML") && line.includes("Rust")));
    assert.match(after, /Markup\./);
    assert.match(after, /Behavior\./);
    assert((await terminal.locator(".terminal-output").innerText()).trimEnd().endsWith("╯"));
    await terminal.getByRole("button", { name: "Toggle layout" }).click();
    assert.equal(await terminal.innerText(), before);
  },
  async Menu(page, terminal) {
    assert.match(await terminal.innerText(), /Selected: Build/);
    await terminal.getByRole("button", { name: "Test", exact: true }).focus();
    await page.keyboard.press("Enter");
    assert.match(await terminal.innerText(), /Selected: Test/);
    await terminal.getByRole("button", { name: "Deploy", exact: true }).click();
    assert.match(await terminal.innerText(), /Selected: Deploy/);
  },
};

await checkBrowser("apps/landing", async (page, origin) => {
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto(origin);
  await page.getByRole("heading", { name: "Write CLI applications using plain HTML" }).waitFor();
  const picker = page.getByRole("group", { name: "Choose an example" });
  await picker.getByRole("button", { name: "Counter", exact: true }).waitFor();
  assert.deepEqual(
    await picker.getByRole("button").allTextContents(), examples.map(([label]) => label),
  );
  assert.equal(
    await picker.getByRole("button", { name: "Counter" }).getAttribute("aria-pressed"), "true",
  );
  await checkTransitions(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  for (const [label, filename] of examples) {
    const terminal = await select(page, label);
    const source = await readFile(resolve(app, "ui", filename), "utf8");
    assert.equal(await (await page.request.get(origin + filename)).text(), source);
    assert.equal((await page.locator(".code-line").allTextContents()).join("\n"), source.trimEnd());
    assert.equal(await terminal.evaluate(node => getComputedStyle(node).animationName), "none");
    await interactions[label](page, terminal);
    assert((await terminal.locator(".terminal-output").innerText()).trimEnd().endsWith("╯"));
    await page.screenshot({
      path: resolve(app, `target/landing-${label.toLowerCase()}.png`), fullPage: true,
    });
  }
  await checkInstallation(page);
  for (const width of [390, 320, 768]) {
    await page.setViewportSize({ width, height: 844 });
    for (const [label] of examples) {
      await select(page, label);
      assert(
        await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
        `${label}: ${width}px overflow`,
      );
    }
    const terminal = await select(page, "Counter");
    await interactions.Counter(page, terminal);
    await page.screenshot({ path: resolve(app, `target/landing-${width}.png`), fullPage: true });
  }
});
console.log(
  "PASS: immediate examples, transitions, interactions, clipboard, links and responsive layout",
);

async function select(page, label) {
  await page.getByRole("group", { name: "Choose an example" })
    .getByRole("button", { name: label, exact: true }).click();
  const terminal = page.getByRole("group", { name: `Interactive terminal: ${label}`, exact: true });
  await terminal.waitFor();
  return terminal;
}

async function checkTransitions(page) {
  const entrance = await page.locator(".example").evaluate(node =>
    node.getAnimations().map(animation => animation.effect.getComputedTiming()));
  assert(entrance.length > 0, "the example has an entrance animation");
  assert(entrance.every(timing => timing.delay === 0 && timing.endTime <= 300));
  for (const [label] of examples) {
    await select(page, label);
    const panes = await page.locator(".source, .terminal-screen")
      .evaluateAll(nodes => nodes.map(node => {
        const animations = node.getAnimations({ subtree: true });
        const timings = animations.map(animation => animation.effect.getComputedTiming());
        const rows = [...node.querySelectorAll(".source-row")];
        return {
          timings,
          visible: getComputedStyle(node).visibility === "visible",
          complete: rows.every(row => getComputedStyle(row).opacity === "1"),
        };
      }));
    for (const pane of panes) {
      assert(pane.timings.length > 0, label + " content has a switching animation");
      assert(pane.timings.every(timing => timing.delay === 0 && timing.endTime <= 300));
      assert(pane.visible && pane.complete, label + " shows its complete HTML and CLI together");
    }
  }
}

async function checkInstallation(page) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.getByRole("button", { name: "Copy command", exact: true }).click();
  await page.getByRole("button", { name: "Copied!", exact: true }).waitFor();
  assert.equal(
    await page.evaluate(() => navigator.clipboard.readText()), "cargo install hypercmd-cli --locked",
  );
  assert.equal(
    await page.getByRole("link", { name: "Documentation" }).getAttribute("href"), "/docs/",
  );
  assert.equal(
    await page.getByRole("link", { name: "GitHub" }).getAttribute("href"),
    "https://github.com/fusor-rs/hypercmd",
  );
  assert.equal(
    await page.getByRole("link", { name: /@fusor_rs/ }).getAttribute("href"), "https://x.com/fusor_rs",
  );
}
