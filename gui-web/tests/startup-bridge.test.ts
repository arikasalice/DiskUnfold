import { afterEach, expect, test, vi } from "vitest";
import {
  hasDesktopLocaleBridge,
  hasStartupStatusBridge,
} from "../src/ipc/bridge";

afterEach(() => vi.unstubAllGlobals());

function stubNativeWindow(href: string, hasHandler = true, mainFrame = true) {
  const host: Record<string, unknown> = {
    location: { href, origin: "diskunfold://app" },
  };
  Object.defineProperty(host, "top", {
    configurable: true,
    value: mainFrame ? host : {},
  });
  if (hasHandler) {
    host.webkit = {
      messageHandlers: { diskunfold: { postMessage: vi.fn() } },
    };
  }
  vi.stubGlobal("window", host);
}

test("detects the trusted Mac startup bridge without claiming locale support", () => {
  stubNativeWindow("diskunfold://app/index.html");

  expect(hasStartupStatusBridge()).toBe(true);
  expect(hasDesktopLocaleBridge()).toBe(false);
});

test("accepts the Mac locale bridge only when the host injects its capability marker", () => {
  stubNativeWindow("diskunfold://app/index.html");
  const host = window as unknown as { __diskunfoldLocale?: boolean };

  host.__diskunfoldLocale = true;
  expect(hasDesktopLocaleBridge()).toBe(true);
  host.__diskunfoldLocale = false;
  expect(hasDesktopLocaleBridge()).toBe(false);
});

test.each([
  ["an external document", "https://example.invalid/index.html", true, true],
  ["a nested frame", "diskunfold://app/index.html", true, false],
  [
    "a page without the reply handler",
    "diskunfold://app/index.html",
    false,
    true,
  ],
])("rejects %s from the Mac startup bridge", (_, href, handler, mainFrame) => {
  stubNativeWindow(href, handler, mainFrame);

  expect(hasStartupStatusBridge()).toBe(false);
});
