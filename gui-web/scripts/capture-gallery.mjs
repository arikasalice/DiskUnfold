// Captures preview gallery surfaces in headless Edge or Chrome, not WebView2.
// The scan-lanes surface also covers Dark at 700 px, an expanded row and one lane.
//
// - history: the operation history item results (F02 README #170), and the full
//   location shown to a pointer resting 0.4 s on the cell cut in the middle;
//   then the details of the lower-bound item (#204).
// - history-list: scope roots on the operation history list and in operation
//   details (#200), including a record from before roots were recorded.
// - browse: the browse table with every mark of the "Scan status" column (#133).
// - date-picker: the shared date field in empty, selected, invalid, open, and
//   forced-colors states (#272).
//   The default browse run captures the #133 scene and both #226 lower-bound scenes.
// - scan-lanes scenes: the F02 #317 collection-condition states and the
//   accepted resource change while the current folder is still being read,
//   plus the F02 #257 lane failure link and issue filters.
//
//   npm run capture:history-gallery -- [output folder]
//   npm run capture:history-list-gallery -- [output folder]
//   npm run capture:browse-gallery -- [output folder] [scene]
//     scene: scan-status | lower-bounds | lower-bounds-notice
//   npm run capture:date-picker-gallery -- [output folder]
//   node scripts/capture-gallery.mjs target-picker <output folder> [scene]
//     scene: default | checking | hidden-all   (F02 #350)
//   node scripts/capture-gallery.mjs scan-lanes <output folder> <scene>
//     scene: scope-collapsed | scope-expanded | scope-error-known |
//            scope-error-unknown | progress | progress-limit-error |
//            failure-lanes | failure-list | failure-all-problems |
//            failure-narrow | failure-dark
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
  // 4 + 1 + 2 location cells: the main record and the two count sentence records.
  history: { ready: ".du-history-item-location", count: 7, pointer: true },
  "history-list": { ready: ".du-history-operation", count: 4, pointer: true },
  browse: { ready: ".du-browse-grid__row", count: 3, pointer: false },
  "scan-lanes": { ready: ".du-scan-lanes", count: 1, pointer: false },
  "date-picker": {
    ready: ".du-date-picker__control",
    count: 4,
    pointer: false,
  },
  "job-center": { ready: ".du-job-center__header", count: 1, pointer: false },
  "history-retention": {
    ready: ".du-history-retention__header",
    count: 1,
    pointer: false,
  },
  // F06 README 51 (#349): the items not drawn and the legend under long counts.
  "space-undrawn": { ready: ".du-undrawn-line", count: 1, pointer: false },
  // F06 README 52 (#297): a histogram interval opens its file list.
  "histogram-list": {
    ready: ".du-histogram-list-preview .du-chart-and-legend__chart svg",
    count: 1,
    pointer: false,
  },
  // F02 #350: the location picker with a network volume and 9 hidden volumes.
  "target-picker": { ready: ".du-drive-picker", count: 1, pointer: false },
  // F06 README 54 (#298): folders divided by type, as bars and as a treemap.
  // Every scene draws the three folders' slices.
  "type-slices": { ready: ".du-slice-stack", count: 3, pointer: false },
};
const surface = process.argv[2];
if (!Object.hasOwn(surfaces, surface))
  throw new Error("Name a surface: " + Object.keys(surfaces).join(", "));
const { ready, count, pointer } = surfaces[surface];
const browseScenes = ["scan-status", "lower-bounds", "lower-bounds-notice"];
const scanLaneScenes = [
  "scope-collapsed",
  "scope-expanded",
  "scope-error-known",
  "scope-error-unknown",
  "progress",
  "progress-limit-error",
  "failure-lanes",
  "failure-list",
  "failure-all-problems",
  "failure-narrow",
  "failure-dark",
];
const requestedBrowseScene = surface === "browse" ? process.argv[4] : undefined;
const targetPickerScenes = ["default", "checking", "hidden-all"];
const requestedScanLaneScene =
  surface === "scan-lanes" ? process.argv[4] : undefined;
const selectedBrowseScene =
  requestedBrowseScene === "notice"
    ? "lower-bounds-notice"
    : requestedBrowseScene;
if (selectedBrowseScene && !browseScenes.includes(selectedBrowseScene))
  throw new Error("Unknown browse scene: " + selectedBrowseScene);
if (requestedScanLaneScene && !scanLaneScenes.includes(requestedScanLaneScene))
  throw new Error("Unknown scan-lanes scene: " + requestedScanLaneScene);
const captureScenes =
  surface === "browse"
    ? selectedBrowseScene
      ? [selectedBrowseScene]
      : browseScenes
    : surface === "scan-lanes" && requestedScanLaneScene
      ? [requestedScanLaneScene]
      : surface === "target-picker"
        ? process.argv[4]
          ? [process.argv[4]]
          : targetPickerScenes
        : [undefined];
const output = resolve(
  process.argv[3] ?? join(gui, "..", "quality-output", "gallery-" + surface),
);
const height = 900;
const scanFailureShots = {
  "failure-lanes": [
    { name: "failure-lanes-light-1280", theme: "light", width: 1280 },
  ],
  "failure-list": [
    { name: "failure-list-light-1280", theme: "light", width: 1280 },
  ],
  "failure-all-problems": [
    {
      name: "failure-all-problems-light-1280",
      theme: "light",
      width: 1280,
    },
  ],
  "failure-narrow": [
    { name: "failure-list-light-700", theme: "light", width: 700 },
  ],
  "failure-dark": [
    { name: "failure-list-dark-1280", theme: "dark", width: 1280 },
  ],
};
const defaultGalleryShots =
  surface === "history-retention"
    ? [
        {
          name: "panel-light-1280",
          theme: "light",
          width: 1280,
          scene: "panel",
        },
        { name: "panel-dark-1280", theme: "dark", width: 1280, scene: "panel" },
        { name: "panel-light-700", theme: "light", width: 700, scene: "panel" },
        {
          name: "history-with-tasks-light-1280",
          theme: "light",
          width: 1280,
          scene: "tasks-panel",
        },
        {
          name: "11-job-center-over-history-light-1280x800",
          theme: "light",
          width: 1280,
          height: 800,
          scene: "job-center-over-history",
        },
        {
          name: "11-job-center-over-history-light-700x800",
          theme: "light",
          width: 700,
          height: 800,
          scene: "job-center-over-history",
        },
        {
          name: "header-auto-cleanup-on",
          theme: "light",
          width: 1280,
          scene: "header",
        },
        {
          name: "enable-confirmation",
          theme: "light",
          width: 1280,
          scene: "enable",
        },
        {
          name: "zero-target-confirmation",
          theme: "light",
          width: 1280,
          scene: "zero",
        },
        {
          name: "count-unavailable",
          theme: "light",
          width: 1280,
          scene: "count-unavailable",
        },
        { name: "stale-recount", theme: "light", width: 1280, scene: "stale" },
        {
          name: "save-failure",
          theme: "light",
          width: 1280,
          scene: "save-failure",
        },
        {
          name: "unreadable-policy",
          theme: "light",
          width: 1280,
          scene: "unreadable",
        },
        {
          name: "forced-colors-confirmation",
          theme: "light",
          width: 1280,
          scene: "enable",
          forcedColors: "active",
        },
      ]
    : surface === "scan-lanes" && requestedScanLaneScene === "scope-collapsed"
      ? [
          {
            name: "scope-collapsed-light-1280",
            theme: "light",
            width: 1280,
            height: 720,
          },
          {
            name: "scope-collapsed-dark-1280",
            theme: "dark",
            width: 1280,
            height: 720,
          },
          {
            name: "scope-collapsed-light-700",
            theme: "light",
            width: 700,
            height: 720,
          },
          {
            name: "scope-collapsed-dark-700",
            theme: "dark",
            width: 700,
            height: 720,
          },
        ]
      : surface === "scan-lanes" && requestedScanLaneScene === "scope-expanded"
        ? [
            {
              name: "scope-expanded-light-1280",
              theme: "light",
              width: 1280,
              height: 720,
            },
            {
              name: "scope-expanded-dark-1280",
              theme: "dark",
              width: 1280,
              height: 720,
            },
            {
              name: "scope-expanded-light-700",
              theme: "light",
              width: 700,
              height: 720,
            },
            {
              name: "scope-expanded-dark-700",
              theme: "dark",
              width: 700,
              height: 720,
            },
          ]
        : surface === "scan-lanes" &&
            requestedScanLaneScene === "scope-error-known"
          ? [
              {
                name: "limit-error-known-light-1280",
                theme: "light",
                width: 1280,
                height: 720,
              },
            ]
          : surface === "scan-lanes" &&
              requestedScanLaneScene === "scope-error-unknown"
            ? [
                {
                  name: "limit-error-unknown-light-1280",
                  theme: "light",
                  width: 1280,
                  height: 720,
                },
              ]
            : surface === "scan-lanes" && requestedScanLaneScene === "progress"
              ? [
                  {
                    name: "progress-light-1280",
                    theme: "light",
                    width: 1280,
                    height: 900,
                  },
                ]
              : surface === "histogram-list"
                ? [
                    {
                      name: "00-chart-light-1280",
                      theme: "light",
                      width: 1280,
                    },
                    {
                      name: "01-bar-list-light-1280",
                      theme: "light",
                      width: 1280,
                      step: "bar",
                    },
                    {
                      name: "02-back-focus-light-1280",
                      theme: "light",
                      width: 1280,
                      step: "back",
                    },
                    {
                      name: "03-table-list-light-1280",
                      theme: "light",
                      width: 1280,
                      step: "table",
                    },
                    {
                      name: "05-unread-light-1280",
                      theme: "light",
                      width: 1280,
                      scene: "unread",
                    },
                    {
                      name: "06-unread-view-in-list-light-1280",
                      theme: "light",
                      width: 1280,
                      scene: "unread",
                      viewInList: true,
                      focusRow: "2",
                    },
                    {
                      name: "05-unread-dark-1280",
                      theme: "dark",
                      width: 1280,
                      scene: "unread",
                    },
                  ]
                : surface === "space-undrawn"
                  ? [
                      { name: "01-light-1280", theme: "light", width: 1280 },
                      {
                        name: "02-view-in-list-light-1280",
                        theme: "light",
                        width: 1280,
                        viewInList: true,
                      },
                      {
                        name: "03-legend-light-1130",
                        theme: "light",
                        width: 1130,
                      },
                      { name: "01-dark-1280", theme: "dark", width: 1280 },
                      {
                        name: "04-partial-light-1280",
                        theme: "light",
                        width: 1280,
                        scene: "partial",
                      },
                    ]
                  : surface === "type-slices"
                    ? [
                        {
                          name: "01-bars-light-1280",
                          theme: "light",
                          width: 1280,
                          scene: "bar",
                        },
                        {
                          name: "02-bars-video-light-1280",
                          theme: "light",
                          width: 1280,
                          scene: "bar",
                          legend: "동영상",
                        },
                        {
                          name: "07-bars-preview-video-light-1280",
                          theme: "light",
                          width: 1280,
                          scene: "bar",
                          hover: "동영상",
                        },
                        {
                          name: "03-treemap-light-1280",
                          theme: "light",
                          width: 1280,
                          scene: "treemap",
                        },
                        {
                          name: "04-table-light-1280",
                          theme: "light",
                          width: 1280,
                          scene: "treemap",
                          list: true,
                        },
                        {
                          name: "05-bars-forced-colors-1280",
                          theme: "light",
                          width: 1280,
                          scene: "bar",
                          forcedColors: "active",
                        },
                        {
                          name: "06-treemap-forced-colors-1280",
                          theme: "light",
                          width: 1280,
                          scene: "treemap",
                          forcedColors: "active",
                        },
                        {
                          name: "03-treemap-dark-1280",
                          theme: "dark",
                          width: 1280,
                          scene: "treemap",
                        },
                      ]
                    : surface === "job-center"
                      ? [
                          {
                            name: "01-light-1280",
                            theme: "light",
                            width: 1280,
                            scene: "main",
                          },
                          {
                            name: "01-dark-1280",
                            theme: "dark",
                            width: 1280,
                            scene: "main",
                          },
                          {
                            name: "01-light-700",
                            theme: "light",
                            width: 700,
                            scene: "main",
                          },
                          {
                            name: "02-request-pending-no-effect",
                            theme: "light",
                            width: 1280,
                            scene: "request-pending",
                          },
                          {
                            name: "03-resync",
                            theme: "light",
                            width: 1280,
                            scene: "resync",
                          },
                          {
                            name: "04-empty",
                            theme: "light",
                            width: 1280,
                            scene: "empty",
                          },
                          {
                            name: "05-cancel-confirm",
                            theme: "light",
                            width: 1280,
                            scene: "cancel-confirm",
                          },
                          {
                            name: "06-forced-colors",
                            theme: "light",
                            width: 1280,
                            scene: "forced-colors",
                            forcedColors: "active",
                          },
                          {
                            name: "07-toast-open-panel-700",
                            theme: "light",
                            width: 700,
                            scene: "toast-open-panel",
                          },
                          {
                            name: "07-toast-open-panel-1280",
                            theme: "light",
                            width: 1280,
                            scene: "toast-open-panel",
                          },
                        ]
                      : [
                          { name: "light-1280", theme: "light", width: 1280 },
                          { name: "dark-1280", theme: "dark", width: 1280 },
                          { name: "light-700", theme: "light", width: 700 },
                        ];
const galleryShots =
  scanFailureShots[requestedScanLaneScene] ?? defaultGalleryShots;
const shots =
  surface === "scan-lanes" && requestedScanLaneScene === "progress-limit-error"
    ? [
        {
          name: "progress-limit-error-light-1280",
          theme: "light",
          width: 1280,
          height: 720,
          locale: "ko",
        },
        {
          name: "progress-limit-error-light-1280",
          theme: "light",
          width: 1280,
          height: 720,
          locale: "en",
        },
      ]
    : surface === "target-picker"
      ? [...galleryShots, { name: "light-360", theme: "light", width: 360 }]
      : galleryShots;

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

async function waitForRetention(session, selector, label) {
  const ready = await session.run(async (query) => {
    for (let tries = 0; tries < 200; tries++) {
      if (document.querySelector(query)) return true;
      await new Promise((done) => setTimeout(done, 50));
    }
    return false;
  }, selector);
  if (!ready) throw new Error("The retention preview did not show " + label);
}

async function prepareRetentionScene(session, scene) {
  if (scene === "header") return;
  if (scene === "job-center-over-history") {
    if ((await session.run(drawn, ".du-job-center[data-open]", 1)) !== 1)
      throw new Error("The Job Center did not open over the history screen");
    if (
      await session.run(() =>
        Boolean(document.querySelector(".du-history-retention[data-open]")),
      )
    )
      throw new Error("The retention panel remained open with the Job Center");
    return;
  }
  await session.run(() => {
    const button = [...document.querySelectorAll("button")].find(
      (item) => item.textContent?.trim() === "보존 설정",
    );
    if (!button) throw new Error("The retention settings button was not found");
    button.click();
  });
  await waitForRetention(
    session,
    ".du-history-retention[data-open]",
    "the panel",
  );
  if (scene === "panel" || scene === "tasks-panel") return;
  if (scene === "unreadable") {
    await waitForRetention(
      session,
      ".du-history-retention .du-notice--warning",
      "the policy warning",
    );
    return;
  }
  await session.run(() => {
    const toggle = document.querySelector('[role="switch"]');
    if (!(toggle instanceof HTMLElement))
      throw new Error("The cleanup switch was not found");
    toggle.click();
  });
  if (scene === "count-unavailable") {
    await waitForRetention(
      session,
      ".du-history-retention .du-notice--error",
      "the count error",
    );
    return;
  }
  await waitForRetention(
    session,
    '[role="alertdialog"]',
    "the confirmation dialog",
  );
  if (scene === "stale" || scene === "save-failure") {
    await session.run(() => {
      const button = [
        ...document.querySelectorAll('[role="alertdialog"] button'),
      ].find((item) => item.textContent?.trim() === "켜기");
      if (!button) throw new Error("The confirm button was not found");
      button.click();
    });
    await waitForRetention(
      session,
      scene === "stale"
        ? ".du-history-retention__dialog .du-notice"
        : ".du-history-retention__dialog .du-notice--error",
      scene === "stale" ? "the refreshed counts" : "the save error",
    );
  }
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

/* In the page: wait for finite motion to finish, then paint two settled frames. */
async function settleAnimations() {
  const nextFrame = () => new Promise(requestAnimationFrame);
  let quietFrames = 0;
  for (;;) {
    const running = document
      .getAnimations()
      .filter(
        (animation) =>
          animation.playState === "running" &&
          Number.isFinite(animation.effect?.getComputedTiming().endTime),
      );
    if (running.length) {
      quietFrames = 0;
      await Promise.allSettled(running.map((animation) => animation.finished));
      continue;
    }
    await nextFrame();
    quietFrames++;
    if (quietFrames === 2) return;
  }
}

/* In the page: the point of the second location, which is cut in the middle. */
function cutCell(surface) {
  const selector =
    surface === "history-list"
      ? ".du-history-operation .du-fit"
      : ".du-history-item-location .du-fit";
  const cell = document.querySelectorAll(selector)[1];
  cell.scrollIntoView({ block: "center" });
  if (!cell.querySelector(".du-fit__full")) return null;
  const box = cell.getBoundingClientRect();
  return {
    x: Math.round(box.left + Math.min(box.width / 2, 80)),
    y: Math.round(box.top + box.height / 2),
  };
}

/** Opens the gallery's surface in a theme and waits until it is drawn. */
async function open(
  session,
  base,
  theme,
  width,
  scene,
  query = "",
  forcedColors = "none",
  viewportHeight = height,
  locale = "ko",
) {
  await session.send("Emulation.setDeviceMetricsOverride", {
    width,
    height: viewportHeight,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await session.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: theme },
      { name: "forced-colors", value: forcedColors },
    ],
  });
  const loaded = session.once("Page.loadEventFired");
  await session.send("Page.navigate", {
    url:
      base +
      "?surface=" +
      surface +
      "&theme=" +
      theme +
      (scene ? "&scene=" + scene : "") +
      (locale === "en" ? "&locale=en" : "") +
      query,
  });
  await loaded;
  if (surface === "scan-lanes") {
    const scopeScene = scene?.startsWith("scope-");
    const selector = scopeScene
      ? ".du-scope-preview .du-collection-conditions"
      : scene === "one-lane"
        ? ".du-scan-lanes__single"
        : ".du-scan-lanes .du-data-table tbody tr";
    const expected = scopeScene ? 1 : scene === "one-lane" ? 1 : 3;
    if ((await session.run(drawn, selector, expected)) !== expected)
      throw new Error(
        "The scan lanes were not drawn for " + (scene ?? "three lanes"),
      );
    if (scopeScene) {
      await session.run((narrow) => {
        const selector = narrow
          ? ".du-scope-preview__summary"
          : ".du-scope-preview";
        document
          .querySelector(selector)
          ?.scrollIntoView({ block: "center", behavior: "auto" });
      }, width <= 760);
    }
  } else {
    const expected =
      surface === "browse" && scene === "scan-status"
        ? 6
        : // The bars add the Other group's slices to the three folders'.
          surface === "type-slices" && scene === "bar"
          ? 4
          : count;
    if ((await session.run(drawn, ready, expected)) !== expected)
      throw new Error(
        "The " + surface + " surface was not drawn for " + (scene ?? "default"),
      );
  }
}

/** Opens the third date field with keyboard input so its focused day is visible. */
async function openDatePickerCalendar(session) {
  await session.run(() => {
    const trigger = document
      .querySelectorAll(".du-date-picker")[2]
      ?.querySelector(
        ".du-date-picker__control .du-date-picker__button:not(.du-date-picker__clear)",
      );
    if (!(trigger instanceof HTMLButtonElement))
      throw new Error("The date-picker preview trigger was not found");
    trigger.focus();
    trigger.click();
  });
  await session.send("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: "ArrowRight",
    code: "ArrowRight",
    windowsVirtualKeyCode: 39,
  });
  await session.send("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: "ArrowRight",
    code: "ArrowRight",
    windowsVirtualKeyCode: 39,
  });
  await sleep(150);
  if (
    !(await session.run(() =>
      Boolean(document.querySelector(".du-date-picker__picker[data-open]")),
    ))
  )
    throw new Error("The date-picker calendar did not open");
}

/**
 * #297: presses the third interval's bar with the mouse, then goes back; or opens
 * the same list from the table's button. Checks the list and the returned focus.
 */
async function histogramListStep(session, step) {
  await session.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  if (step === "table") {
    const opened = await session.run(function openFromTable() {
      const fold = document.querySelector(
        ".du-histogram-list-preview .du-detail-disclosure button",
      );
      fold?.click();
      return Boolean(fold);
    });
    if (!opened) throw new Error("The interval table was not found");
    await sleep(200);
    const pressed = await session.run(function pressTableButton() {
      const buttons = Array.from(
        document.querySelectorAll(
          ".du-histogram-list-preview table .du-action",
        ),
      );
      const button = buttons[2];
      button?.click();
      return Boolean(button);
    });
    if (!pressed) throw new Error("The interval's table button was not found");
  } else {
    const point = await session.run(function barCentre() {
      const bars = document.querySelectorAll(
        ".du-histogram-list-preview [aria-label*='목록에서 보기']",
      );
      const box = bars[2]?.getBoundingClientRect();
      return box
        ? { x: box.x + box.width / 2, y: box.y + box.height / 2 }
        : null;
    });
    if (!point) throw new Error("The interval bar was not found");
    for (const type of ["mousePressed", "mouseReleased"])
      await session.send("Input.dispatchMouseEvent", {
        type,
        ...point,
        button: "left",
        clickCount: 1,
      });
  }
  if ((await session.run(drawn, ".du-type-list tbody tr", 6)) < 6)
    throw new Error("The interval's list did not open");
  if (step !== "back") return;
  await session.run(function goBack() {
    document.querySelector(".du-type-list .du-action")?.click();
  });
  await sleep(200);
  const back = await session.run(
    () => document.activeElement?.getAttribute("aria-label") ?? "",
  );
  if (!back.includes("목록에서 보기"))
    throw new Error("The focus did not return to the bar: " + back);
}

/**
 * The checking mark turns, and stands still with reduced motion from the OS or from
 * the app setting (F02 #350).
 */
async function checkProgressMarkMotion(session) {
  const animation = () =>
    session.run(
      () =>
        getComputedStyle(
          document.querySelector(
            '.du-badge[data-status-shape="progress"] .du-badge__shape',
          ),
        ).animationName,
    );
  if ((await animation()) === "none")
    throw new Error("The checking mark does not turn");
  await session.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: "light" },
      { name: "prefers-reduced-motion", value: "reduce" },
    ],
  });
  if ((await animation()) !== "none")
    throw new Error("The checking mark turns with reduced motion from the OS");
  await session.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: "light" },
      { name: "prefers-reduced-motion", value: "no-preference" },
    ],
  });
  await session.run(() => {
    document.documentElement.dataset.duMotion = "reduce";
  });
  if ((await animation()) !== "none")
    throw new Error("The checking mark turns with reduced motion in the app");
  console.log("checking mark: turns, and stands still with reduced motion");
}

/** Saves the whole page, or with `viewport` only what the window shows. */
async function save(session, name, viewport = false, scene) {
  await session.run(settleAnimations);
  if (surface === "job-center" || surface === "history-retention") {
    const width = await session.run(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    if (width.scrollWidth !== width.clientWidth)
      throw new Error(
        `Horizontal overflow in ${surface}/${scene ?? name}: document width ${width.scrollWidth}px exceeds viewport ${width.clientWidth}px`,
      );
    console.log(
      `${surface}/${scene ?? name}: document width ${width.scrollWidth}/${width.clientWidth}px`,
    );
  }
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
    "gallery-" +
      surface +
      (scene ? "-" + scene : "") +
      "-browser-ko-" +
      name +
      ".png",
  );
  await writeFile(file, Buffer.from(data, "base64"));
  console.log(file);
}

/** Captures only the product progress header, without gallery controls. */
async function saveLanes(session, name, locale = "ko") {
  const clip = await session.run(function headerClip() {
    const element = document.querySelector(
      ".du-scan-lanes-preview .du-result__header",
    );
    if (!element) return null;
    const box = element.getBoundingClientRect();
    return {
      x: Math.floor(box.x),
      y: Math.floor(box.y),
      width: Math.ceil(box.width),
      height: Math.ceil(box.height),
    };
  });
  if (!clip) throw new Error("The progress header was not found");
  const { data } = await session.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: true,
    clip: { ...clip, scale: 1 },
  });
  const file = join(
    output,
    "gallery-scan-lanes-browser-" + locale + "-" + name + ".png",
  );
  await writeFile(file, Buffer.from(data, "base64"));
  console.log(file);
}

async function saveFailureScene(session, name) {
  await save(session, name, true);
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
  for (const scene of captureScenes) {
    for (const shot of shots) {
      await open(
        session,
        base,
        shot.theme,
        shot.width,
        shot.scene ?? scene,
        "",
        shot.forcedColors,
        shot.height ?? height,
        shot.locale ?? "ko",
      );
      if (surface === "scan-lanes" && scene?.startsWith("scope-"))
        await save(session, shot.name, true, scene);
      else if (surface === "scan-lanes" && scene?.startsWith("failure-")) {
        if (scene !== "failure-lanes") {
          const activated = await session.run(() => {
            const link = document.querySelector(
              '.du-scan-lanes .du-text-link[aria-label="읽지 못한 위치 2곳 보기"]',
            );
            if (!(link instanceof HTMLElement)) return false;
            link.click();
            return true;
          });
          if (!activated)
            throw new Error("The lane failure link was not found");
          if ((await session.run(drawn, ".du-scan-issues__chip", 1)) !== 1)
            throw new Error("The storage filter chip was not drawn");
          let focusedTitle = false;
          for (let attempt = 0; attempt < 40 && !focusedTitle; attempt += 1) {
            await sleep(50);
            focusedTitle = await session.run(() => {
              const heading = document.querySelector(
                ".du-scan-issues .du-heading",
              );
              return (
                heading instanceof HTMLElement &&
                document.activeElement === heading &&
                heading.getAttribute("aria-label")?.includes("2")
              );
            });
          }
          if (!focusedTitle)
            throw new Error("The filtered scan issues title was not focused");
          const unreadableSelected = await session.run(() => {
            const select = document.querySelector(".du-scan-issues select");
            return (
              select instanceof HTMLSelectElement &&
              select.value === "unreadable_locations"
            );
          });
          if (!unreadableSelected)
            throw new Error("The unreadable locations option was not selected");
          if (scene === "failure-all-problems") {
            const changed = await session.run(() => {
              const select = document.querySelector(".du-scan-issues select");
              if (!(select instanceof HTMLSelectElement)) return false;
              Object.getOwnPropertyDescriptor(
                HTMLSelectElement.prototype,
                "value",
              )?.set?.call(select, "all");
              select.dispatchEvent(new Event("change", { bubbles: true }));
              return true;
            });
            if (!changed)
              throw new Error("The Problem type selector was not found");
            const allSelected = await session.run(() => {
              const select = document.querySelector(".du-scan-issues select");
              return (
                select instanceof HTMLSelectElement &&
                select.value === "all" &&
                select.selectedOptions[0]?.textContent === "모든 문제"
              );
            });
            if (!allSelected)
              throw new Error("The All problems option was not selected");
            if ((await session.run(drawn, ".du-scan-issues__chip", 1)) !== 1)
              throw new Error("The Storage filter chip did not remain");
          }
          if (scene === "failure-narrow") {
            const singleStorageChip = await session.run(() => {
              const chips = document.querySelectorAll(".du-scan-issues__chip");
              return chips.length === 1;
            });
            if (!singleStorageChip)
              throw new Error("The storage chip was not shown at 700 px");
          }
        }
        await saveFailureScene(session, shot.name);
      } else if (surface === "scan-lanes" && scene === "progress") {
        const changed = await session.run(() => {
          const fast = document.querySelector(
            'input[type="radio"][value="fast"]',
          );
          if (!(fast instanceof HTMLElement)) return false;
          const choice = fast.closest("label");
          if (!(choice instanceof HTMLElement)) return false;
          choice.click();
          return true;
        });
        if (!changed) throw new Error("The Fast resource option was not found");
        if (
          (await session.run(drawn, ".du-scan-lane__resource-pending", 1)) < 1
        )
          throw new Error(
            "The resource change is not waiting on the current folder",
          );
        await saveLanes(session, shot.name);
      } else if (surface === "scan-lanes" && scene === "progress-limit-error") {
        const expanded = await session.run(function expandLimit() {
          const button = document.querySelector(
            ".du-resource-limit__heading button",
          );
          if (!(button instanceof HTMLElement)) return false;
          button.click();
          return true;
        });
        if (!expanded) throw new Error("The limit disclosure was not found");
        if (
          (await session.run(
            drawn,
            ".du-resource-limit__heading button[aria-expanded='true']",
            1,
          )) < 1
        )
          throw new Error("The limit disclosure did not expand");
        if (
          (await session.run(
            drawn,
            ".du-scan-resource-control .du-resource-limit__field input",
            1,
          )) < 1
        )
          throw new Error("The collection limit field did not open");
        const entered = await session.run(function enterLimitError() {
          const input = document.querySelector(
            ".du-scan-resource-control .du-resource-limit__field input",
          );
          if (!(input instanceof HTMLInputElement)) return false;
          const setValue = Object.getOwnPropertyDescriptor(
            HTMLInputElement.prototype,
            "value",
          )?.set;
          setValue?.call(input, "99");
          input.dispatchEvent(new Event("input", { bubbles: true }));
          input.dispatchEvent(new Event("change", { bubbles: true }));
          return input.value === "99";
        });
        if (!entered) throw new Error("The invalid limit was not entered");
        if (
          (await session.run(
            drawn,
            ".du-scan-resource-control [slot='errorMessage']",
            1,
          )) < 1
        )
          throw new Error("The 1 to 16 limit error was not shown");
        const layout = await session.run(function checkLimitErrorLayout() {
          const lineCount = (selector) => {
            const element = document.querySelector(selector);
            if (!element) return 0;
            const range = document.createRange();
            range.selectNodeContents(element);
            return range.getClientRects().length;
          };
          const input = document.querySelector(
            ".du-scan-resource-control .du-resource-limit__field input",
          );
          return {
            inputWidth: input?.getBoundingClientRect().width ?? 0,
            errorLines: lineCount(
              ".du-scan-resource-control [slot='errorMessage']",
            ),
            previousLines: lineCount(
              ".du-scan-resource-control .du-resource-limit__previous",
            ),
          };
        });
        if (
          layout.inputWidth > 144 ||
          layout.inputWidth < 100 ||
          layout.errorLines !== 1 ||
          layout.previousLines !== 1
        )
          throw new Error(
            "Unexpected limit error layout: " + JSON.stringify(layout),
          );
        await saveLanes(session, shot.name, shot.locale);
      } else if (surface === "scan-lanes") await saveLanes(session, shot.name);
      else if (surface === "target-picker") {
        if (!targetPickerScenes.includes(scene))
          throw new Error("Unknown target-picker scene: " + scene);
        if (scene === "hidden-all") {
          const opened = await session.run(function showHiddenVolumes() {
            const toggle = Array.from(document.querySelectorAll("button")).find(
              (button) => button.textContent?.includes("모두 보기"),
            );
            if (!toggle) return false;
            toggle.click();
            return true;
          });
          if (!opened)
            throw new Error("The hidden volumes toggle was not found");
          await sleep(150);
        }
        await save(session, shot.name, false, scene);
        if (scene === "checking" && shot.name === "light-1280")
          await checkProgressMarkMotion(session);
      } else if (surface === "history-retention") {
        await prepareRetentionScene(session, shot.scene ?? "panel");
        await save(session, shot.name, true);
      } else if (surface === "job-center") await save(session, shot.name, true);
      else if (surface === "type-slices" && shot.hover) {
        // The pointer rests on a legend row (README 46.1, #383): a real mouse move.
        const box = await session.run(function legendRowBox(name) {
          const row = Array.from(
            document.querySelectorAll(".du-chart-legend [role='option']"),
          ).find((option) => option.textContent?.startsWith(name));
          if (!(row instanceof HTMLElement)) return null;
          const rect = row.getBoundingClientRect();
          return {
            x: rect.left + rect.width / 2,
            y: rect.top + rect.height / 2,
          };
        }, shot.hover);
        if (!box) throw new Error("The legend row was not found");
        await session.send("Input.dispatchMouseEvent", {
          type: "mouseMoved",
          x: box.x,
          y: box.y,
        });
        await sleep(300);
        await save(session, shot.name, false);
      } else if (surface === "type-slices" && (shot.legend || shot.list)) {
        const pressed = await session.run(
          function pressTypeControl(legend, list) {
            const target = legend
              ? Array.from(
                  document.querySelectorAll(".du-chart-legend [role='option']"),
                ).find((option) => option.textContent?.startsWith(legend))
              : Array.from(document.querySelectorAll("button")).find((button) =>
                  button.textContent?.includes(list),
                );
            if (!(target instanceof HTMLElement)) return false;
            target.click();
            return true;
          },
          shot.legend ?? null,
          shot.list ? "이 폴더의 항목" : null,
        );
        if (!pressed) throw new Error("The legend class or list was not found");
        await sleep(300);
        await save(session, shot.name, false);
      } else if (surface === "histogram-list" && shot.step) {
        await histogramListStep(session, shot.step);
        await save(session, shot.name, true);
      } else if (surface === "space-undrawn" && shot.viewInList) {
        // A headless page has no focus of its own, so :focus would not paint.
        await session.send("Emulation.setFocusEmulationEnabled", {
          enabled: true,
        });
        const pressed = await session.run(function pressViewInList() {
          const button = Array.from(
            document.querySelectorAll(".du-undrawn-line .du-action"),
          ).find((element) => element.textContent?.trim() === "목록에서 보기");
          if (!(button instanceof HTMLElement)) return false;
          button.click();
          return true;
        });
        if (!pressed) throw new Error("View in list was not found");
        await sleep(300);
        const focused = await session.run(
          () => document.activeElement?.getAttribute("aria-rowindex") ?? "",
        );
        if (focused !== (shot.focusRow ?? "4"))
          throw new Error(
            "The first item not drawn was not focused: " + focused,
          );
        await save(session, shot.name, false);
      } else await save(session, shot.name, false, scene);
    }
  }
  if (surface === "scan-lanes" && !requestedScanLaneScene) {
    await open(session, base, "light", 1280);
    const expanded = await session.run(function expandPriorityLane() {
      const rows = document.querySelectorAll(
        ".du-scan-lanes .du-data-table tbody tr",
      );
      const button = rows[1]?.querySelector(
        "td:first-child button[aria-expanded='false']",
      );
      button?.click();
      return Boolean(button);
    });
    if (!expanded) throw new Error("The priority lane row was not available");
    await session.run(
      drawn,
      ".du-scan-lanes .du-data-table tbody tr:nth-child(2) td:first-child .du-detail-disclosure__content",
      1,
    );
    await saveLanes(session, "light-1280-expanded");
    await open(session, base, "light", 1280, "one-lane");
    await saveLanes(session, "light-1280-one-lane");
  }
  if (surface === "date-picker") {
    await open(session, base, "light", 1280);
    await openDatePickerCalendar(session);
    await save(session, "light-1280-calendar");
    await open(session, base, "light", 1280);
    await session.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-color-scheme", value: "light" },
        { name: "forced-colors", value: "active" },
      ],
    });
    await openDatePickerCalendar(session);
    await save(session, "light-1280-forced-colors-calendar");
  }
  if (pointer) {
    await open(session, base, "light", 1280);
    const point = await session.run(cutCell, surface);
    if (!point) throw new Error("The long location was not cut at 1280 px");
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      ...point,
    });
    await sleep(400);
    await save(session, "light-1280-tooltip", true);
    if (surface === "history") {
      // #204: the details of the lower-bound item show its size with the size rule.
      await open(session, base, "light", 1280);
      await session.run(() => {
        const view = document.querySelectorAll(
          ".du-history-item-results .du-table-cell .du-action",
        );
        view[2]?.scrollIntoView({ block: "center" });
        view[2]?.click();
      });
      await sleep(400);
      await save(session, "light-1280-item-details", true);
    }
    if (surface === "history-list") {
      const openRow = async (rowIndex) => {
        const opened = await session.run((index) => {
          const action = document.querySelector(
            `.du-history-screen:not([hidden]) tr[aria-rowindex="${index}"] .du-action`,
          );
          if (!action) return false;
          action.scrollIntoView({ block: "center" });
          action.click();
          return true;
        }, rowIndex);
        if (!opened) throw new Error("The history row action was not found");
      };
      const backToList = async () => {
        const closed = await session.run(() => {
          const action = Array.from(
            document.querySelectorAll(
              ".du-history-screen:not([hidden]) .du-history-header .du-action",
            ),
          ).find(
            (element) => element.textContent?.trim() === "이력으로 돌아가기",
          );
          if (!action) return false;
          action.click();
          return true;
        });
        if (!closed)
          throw new Error("The back-to-history action was not found");
        if (
          (await session.run(
            drawn,
            ".du-history-screen:not([hidden]) .du-history-operation",
            4,
          )) !== 4
        )
          throw new Error("The history list did not return after details");
      };

      await open(session, base, "light", 1280);
      await openRow(3);
      if (
        (await session.run(
          drawn,
          ".du-history-screen:not([hidden]) .du-history-roots",
          1,
        )) !== 1
      )
        throw new Error("The history detail roots were not drawn");
      await save(session, "light-1280-detail-roots");

      await backToList();
      await open(session, base, "light", 700);
      await openRow(3);
      if (
        (await session.run(
          drawn,
          ".du-history-screen:not([hidden]) .du-history-roots",
          1,
        )) !== 1
      )
        throw new Error("The narrow history detail roots were not drawn");
      await save(session, "light-700-detail-roots");

      await backToList();
      await session.send("Emulation.setDeviceMetricsOverride", {
        width: 1280,
        height,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await openRow(5);
      if (
        (await session.run(
          drawn,
          ".du-history-screen:not([hidden]) .du-history-summary",
          1,
        )) !== 1
      )
        throw new Error("The older history detail was not drawn");
      const notRecorded = await session.run(() =>
        document
          .querySelector(".du-history-screen:not([hidden]) .du-history-summary")
          ?.textContent?.includes("기록되지 않음"),
      );
      if (!notRecorded)
        throw new Error("The older record did not show roots as not recorded");
      await save(session, "light-1280-detail-not-recorded");
    }
  }
  session.close();
} finally {
  browser.kill();
  await server.close();
  await sleep(500);
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}
