// Captures a surface of the preview gallery in a headless Chromium browser (Edge
// or Chrome), not in the app's WebView2: Light and Dark at 1280 px and Light at
// 700 px.
//
// - history: the operation history item results (F02 README #170), and the full
//   location shown to a pointer resting 0.4 s on the cell cut in the middle.
// - browse: the browse table with every mark of the "Scan status" column (#133).
//
//   npm run capture:history-gallery -- [output folder]
//   npm run capture:browse-gallery -- [output folder]
//
// The browser is DU_CAPTURE_BROWSER or the first usual Edge or Chrome install.
// Vite serves the page from preview/vite.config.ts (port 4192). The window
// size, scale, theme and pointer position are fixed and every capture waits
// for the fonts and two frames, so a run draws the same pictures each time on
// the same machine.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const gui = fileURLToPath(new URL("..", import.meta.url));
/** What each surface draws once it is ready, and whether a pointer capture follows. */
const surfaces = {
  history: { ready: ".du-history-item-location", count: 4, pointer: true },
  browse: { ready: ".du-browse-grid__row", count: 6, pointer: false },
};
const surface = process.argv[2];
if (!Object.hasOwn(surfaces, surface))
  throw new Error("Name a surface: " + Object.keys(surfaces).join(", "));
const { ready, count, pointer } = surfaces[surface];
const output = resolve(
  process.argv[3] ?? join(gui, "..", "quality-output", "gallery-" + surface),
);
const height = 900;
const shots = [
  { name: "light-1280", theme: "light", width: 1280 },
  { name: "dark-1280", theme: "dark", width: 1280 },
  { name: "light-700", theme: "light", width: 700 },
];

function browserPath() {
  const named = process.env.DU_CAPTURE_BROWSER;
  const candidates = named
    ? [named]
    : [
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
        "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
        "C:/Program Files/Google/Chrome/Application/chrome.exe",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/usr/bin/microsoft-edge",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
      ];
  const found = candidates.find((path) => existsSync(path));
  if (!found)
    throw new Error("No Edge or Chrome found; set DU_CAPTURE_BROWSER");
  return found;
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** Starts the browser and returns the DevTools port it chose. */
async function launch(profile) {
  const browser = spawn(
    browserPath(),
    [
      "--headless=new",
      "--remote-debugging-port=0",
      "--user-data-dir=" + profile,
      "--no-first-run",
      "--no-default-browser-check",
      "--hide-scrollbars",
      "--force-device-scale-factor=1",
      "--lang=ko-KR",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  const portFile = join(profile, "DevToolsActivePort");
  for (let tries = 0; tries < 200; tries++) {
    if (existsSync(portFile)) {
      const [port] = (await readFile(portFile, "utf8")).split("\n");
      if (port) return { browser, port };
    }
    await sleep(50);
  }
  browser.kill();
  throw new Error("The browser did not open its DevTools port");
}

/** A Chrome DevTools Protocol session on one new page. */
async function page(port) {
  const target = await (
    await fetch("http://127.0.0.1:" + port + "/json/new?about:blank", {
      method: "PUT",
    })
  ).json();
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  let next = 0;
  const pending = new Map();
  const waiting = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id !== undefined) {
      const { done, fail } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) fail(new Error(JSON.stringify(message.error)));
      else done(message.result);
    } else waiting.get(message.method)?.();
  };
  const send = (method, params = {}) =>
    new Promise((done, fail) => {
      const id = ++next;
      pending.set(id, { done, fail });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const once = (method) =>
    new Promise((done) => waiting.set(method, () => done()));
  /** Runs `fn` in the page with JSON arguments and returns its value. */
  const run = async (fn, ...args) => {
    const { result, exceptionDetails } = await send("Runtime.evaluate", {
      expression:
        "(" +
        fn.toString() +
        ")(" +
        args.map((arg) => JSON.stringify(arg)) +
        ")",
      awaitPromise: true,
      returnByValue: true,
    });
    if (exceptionDetails) throw new Error(exceptionDetails.text);
    return result.value;
  };
  return { send, once, run, close: () => socket.close() };
}

/* In the page: waits for `count` elements matching `selector`, the fonts and two frames. */
async function drawn(selector, count) {
  const cells = () => document.querySelectorAll(selector).length;
  for (let tries = 0; tries < 200 && cells() < count; tries++)
    await new Promise((done) => setTimeout(done, 50));
  await document.fonts.ready;
  await new Promise((done) =>
    requestAnimationFrame(() => requestAnimationFrame(done)),
  );
  return cells();
}

/* In the page: the point of the second location, which is cut in the middle. */
function cutCell() {
  const cell = document.querySelectorAll(
    ".du-history-item-location .du-fit",
  )[1];
  cell.scrollIntoView({ block: "center" });
  if (!cell.querySelector(".du-fit__full")) return null;
  const box = cell.getBoundingClientRect();
  return {
    x: Math.round(box.left + Math.min(box.width / 2, 80)),
    y: Math.round(box.top + box.height / 2),
  };
}

/** Opens the gallery's surface in a theme and waits until it is drawn. */
async function open(session, base, theme, width) {
  await session.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await session.send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-color-scheme", value: theme }],
  });
  const loaded = session.once("Page.loadEventFired");
  await session.send("Page.navigate", {
    url: base + "?surface=" + surface + "&theme=" + theme,
  });
  await loaded;
  if ((await session.run(drawn, ready, count)) !== count)
    throw new Error("The " + surface + " surface was not drawn");
}

/** Saves the whole page, or with `viewport` only what the window shows. */
async function save(session, name, viewport = false) {
  const options = { format: "png" };
  if (!viewport) {
    const { cssContentSize } = await session.send("Page.getLayoutMetrics");
    options.captureBeyondViewport = true;
    options.clip = {
      x: 0,
      y: 0,
      width: cssContentSize.width,
      height: cssContentSize.height,
      scale: 1,
    };
  }
  const { data } = await session.send("Page.captureScreenshot", options);
  const file = join(
    output,
    "gallery-" + surface + "-browser-ko-" + name + ".png",
  );
  await writeFile(file, Buffer.from(data, "base64"));
  console.log(file);
}

await mkdir(output, { recursive: true });
const server = await createServer({
  configFile: join(gui, "preview", "vite.config.ts"),
  logLevel: "error",
});
await server.listen();
const base = server.resolvedUrls.local[0];
const profile = await mkdtemp(join(tmpdir(), "du-capture-"));
const { browser, port } = await launch(profile);
try {
  const session = await page(port);
  await session.send("Page.enable");
  for (const shot of shots) {
    await open(session, base, shot.theme, shot.width);
    await save(session, shot.name);
  }
  if (pointer) {
    await open(session, base, "light", 1280);
    const point = await session.run(cutCell);
    if (!point) throw new Error("The long location was not cut at 1280 px");
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      ...point,
    });
    await sleep(400);
    await save(session, "light-1280-tooltip", true);
  }
  session.close();
} finally {
  browser.kill();
  await server.close();
  await sleep(500);
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}
