// Screenshots of the app through the Chrome DevTools Protocol (Node 22+, no dependencies).
//
//   node scripts/screenshots.mjs demo   <out-dir> [light|dark] [canvas|proof]
//       Starts headless Chrome on the browser demo (`npx vite --port 5179` must be running).
//   node scripts/screenshots.mjs app    <out-dir> [light|dark]
//       Drives the running desktop app. Start it with WebView2 remote debugging on:
//       WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 npm run tauri dev
//       (Windows only: WebView2 is what exposes the port.) Canvas look, every project and section.
//
// CHROME overrides the browser path. It sets the look and theme in the page's localStorage.
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [mode, out, theme = "light", look = "canvas"] = process.argv.slice(2);
if (!["demo", "app"].includes(mode) || !out) {
  console.error("usage: screenshots.mjs demo|app <out-dir> [light|dark] [canvas|proof]");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let proc = null;
let port = 9223;
if (mode === "demo") {
  port = 9333;
  const chrome = process.env.CHROME ?? {
    win32: "C:/Program Files/Google/Chrome/Application/chrome.exe",
    darwin: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  }[process.platform] ?? "google-chrome";
  const prof = mkdtempSync(join(tmpdir(), "st-shot-"));
  proc = spawn(chrome, ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
}
let targets;
for (let i = 0; i < 40 && !targets; i++) {
  try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); } catch { await sleep(250); }
}
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d.result); pending.delete(d.id); } });
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const js = async (expr) => (await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }))?.result?.value;
const shot = async (name) => { const r = await send("Page.captureScreenshot", { format: "png" }); writeFileSync(join(out, name), Buffer.from(r.data, "base64")); };
// Matches a button by its text, ignoring a trailing count ("Databases 6").
const click = (text, scope = "") => js(`(()=>{const b=[...document.querySelectorAll('${scope} button')].find(b=>b.textContent.trim().replace(/\\d+$/,"")===${JSON.stringify(text)});if(b){b.click();return true}return false})()`);
const settle = async () => { for (let i = 0; i < 40; i++) { if (!(await js(`!!document.querySelector('.loading') || [...document.querySelectorAll('button')].some(b=>/Reading…/.test(b.textContent))`))) return; await sleep(250); } };

await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
if (mode === "demo") { await send("Page.navigate", { url: "http://localhost:5179/" }); await sleep(1500); }
await js(`localStorage.setItem('sidetrack.look', ${JSON.stringify(look)}); localStorage.setItem('sidetrack.theme', ${JSON.stringify(theme)}); localStorage.setItem('sidetrack.sidebar-collapsed','0'); true`);
await send("Page.reload");
await sleep(2000);
await settle();

const sections = ["Branches", "Databases", "Containers", "Structure"];
if (look === "proof") {
  await shot("all-projects.png");
  await click("Open project"); await sleep(1200); await settle();
  await shot("project.png");
  await click("Back to all projects"); await sleep(800);
  await click("Settings"); await sleep(500);
  await shot("settings.png");
} else {
  const projects = await js(`[...document.querySelectorAll('.side-nav > ul > li > .side-item .side-name')].map(e=>e.textContent)`);
  for (const p of mode === "demo" ? projects.slice(0, 1) : projects) {
    await click(p, ".side-nav"); await sleep(800); await settle();
    await shot(`${p}-overview.png`);
    for (const s of sections) {
      await click(s, ".side-sub"); await sleep(1200); await settle(); await sleep(400);
      await shot(`${p}-${s.toLowerCase()}.png`);
    }
  }
  await click("Settings", ".side-foot"); await sleep(500);
  await shot("settings.png");
}
ws.close();
proc?.kill();
console.log(`saved to ${out}`);
