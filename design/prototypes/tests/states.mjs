// Browser test of the magnesium ranking prototype on every generator case
// (AC17, AC19–AC23). The prototype is rebuilt per case by
// tools/build-prototype.py from design/prototypes/cases/, which the Rust
// test `site-gen --test evaluation` keeps equal to the generator's output.
//
//   cd design/prototypes/tests && npm ci && node states.mjs [--shots DIR]
//
// Uses Playwright's Chromium; set PLAYWRIGHT_BROWSERS_PATH if it is
// installed elsewhere. Exits non-zero on the first failed case.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const casesDir = join(root, "design/prototypes/cases");
const shotsAt = process.argv.indexOf("--shots");
const shots = shotsAt > 0 ? resolve(process.argv[shotsAt + 1]) : null;
if (shots) mkdirSync(shots, { recursive: true });

const tmp = mkdtempSync(join(tmpdir(), "proto-"));
const failures = [];
const fail = (name, msg) => failures.push(`${name}: ${msg}`);

const browser = await chromium.launch();
for (const file of readdirSync(casesDir).filter(f => f.endsWith(".json")).sort()) {
  const name = file.replace(/\.json$/, "");
  const data = JSON.parse(readFileSync(join(casesDir, file), "utf8"));
  const html = join(tmp, `${name}.html`);
  execFileSync("python3", [join(root, "tools/build-prototype.py"), "--case", name, "--out", html], { stdio: "ignore" });

  for (const [vp, size] of [["desktop", { width: 1280, height: 900 }], ["phone", { width: 390, height: 844 }]]) {
    const page = await browser.newPage({ viewport: size });
    const errors = [];
    page.on("pageerror", e => errors.push(e.message));
    // Web fonts may be unreachable offline; that is not the page's error.
    page.on("console", m => { if (m.type() === "error" && !/Failed to load resource/.test(m.text())) errors.push(m.text()); });
    await page.goto("file://" + html);
    await page.waitForTimeout(250);

    const check = async (label) => {
      const text = await page.evaluate(() => document.body.innerText);
      for (const bad of ["NaN", "Infinity", "undefined", "null"]) {
        if (new RegExp(`\\b${bad}\\b`).test(text)) fail(name, `${vp} ${label}: page shows "${bad}"`);
      }
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
      if (overflow > 0) fail(name, `${vp} ${label}: ${overflow}px horizontal overflow`);
    };

    for (const [i, preset] of data.presets.entries()) {
      await page.keyboard.press(String(i + 1));
      await page.waitForTimeout(60);
      const label = `preset ${preset.key}`;
      await check(label);
      // AC23: the rows are in the generator's order, nothing re-sorted.
      const shown = await page.$$eval("#rows > li", lis => lis.map(li => li.dataset.id));
      if (JSON.stringify(shown) !== JSON.stringify(preset.order)) fail(name, `${vp} ${label}: rows ${shown} ≠ generator ${preset.order}`);
      const answer = await page.$eval(".answer", n => n.innerText);
      if (!answer.trim()) fail(name, `${vp} ${label}: empty answer`);
      // AC22: an empty ranking says why.
      if (preset.order.length === 0 && data.category.compared > 0 && !answer.includes(preset.empty)) fail(name, `${vp} ${label}: empty ranking not explained`);
      // AC17: no runner-up without a second product.
      if (preset.order.length < 2 && /#2\b/.test(answer)) fail(name, `${vp} ${label}: mentions #2 with ${preset.order.length} ranked`);
      if (preset.order.length === 1 && /\b(best|least|highest|leads)\b/.test(answer)) fail(name, `${vp} ${label}: claims superiority with one ranked product`);
      // A tie with #2 is said as a tie, not as a lead.
      const tie = preset.lead && Math.abs(preset.lead.value - (preset.lead.kind === "price_ratio" ? 1 : 0)) < 1e-9;
      if (tie && /\b(best|least|highest|leads)\b/.test(answer)) fail(name, `${vp} ${label}: claims a lead on a tie`);
    }
    if (data.category.compared === 0) {
      const map = await page.$eval("#mapEmpty", n => !n.hidden && n.innerText.length > 0);
      if (!map) fail(name, `${vp}: nothing compared but the map does not say so`);
    }

    // AC19: an explicit choice survives a new order.
    const first = data.presets[0], second = data.presets[1];
    if (first && second && first.order.length > 2) {
      await page.keyboard.press("1");
      const pick = first.order[2];
      await page.click(`#rows > li[data-id="${pick}"] .brand`);
      await page.keyboard.press("2");
      await page.waitForTimeout(60);
      const kept = await page.$eval("#rail .r-name .t", n => n.innerText);
      const want = data.products.find(p => p.id === pick).title.split(",")[0];
      if (kept !== want) fail(name, `${vp}: selection ${want} replaced by ${kept} after a new order`);
    }

    if (errors.length) fail(name, `${vp}: ${errors.join(" | ")}`);
    if (shots) {
      await page.keyboard.press("1");
      await page.screenshot({ path: join(shots, `${name}-${vp}.png`), fullPage: vp === "desktop" });
    }
    await page.close();
  }
  console.log(`${failures.some(f => f.startsWith(name + ":")) ? "FAIL" : "ok  "} ${name}`);
}
await browser.close();

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("all prototype cases render from the generator's output without errors");
