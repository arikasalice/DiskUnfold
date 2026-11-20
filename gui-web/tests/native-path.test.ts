import { afterEach, expect, test, vi } from "vitest";
import { nativePath, unixPath, windowsPath } from "../src/ipc";

/*
 * #253: a typed or dropped path is encoded the way the host opens paths. The macOS
 * and Linux hosts open raw bytes, so their paths must not be sent as UTF-16.
 */

afterEach(() => vi.unstubAllGlobals());

function decoded(raw: string): number[] {
  const base64 = raw.replaceAll("-", "+").replaceAll("_", "/");
  return [...atob(base64)].map((char) => char.charCodeAt(0));
}

test("a Unix path keeps its UTF-8 bytes and its display", () => {
  const path = unixPath("/Users/박");

  expect(path.encoding).toBe("unix");
  expect(path.display).toBe("/Users/박");
  expect(path.lossy).toBe(false);
  expect(decoded(path.raw)).toEqual([...new TextEncoder().encode("/Users/박")]);
});

test("a lone surrogate is replaced and marks the Unix path lossy", () => {
  const path = unixPath("/a\uD800");

  expect(path.display).toBe("/a\uFFFD");
  expect(path.lossy).toBe(true);
});

test("the WebKit hosts of macOS and Linux get Unix paths", () => {
  vi.stubGlobal("webkit", {
    messageHandlers: { diskunfold: { postMessage: vi.fn() } },
  });

  expect(nativePath("/Users/park")).toEqual(unixPath("/Users/park"));
});

test("WebView2 on Windows, or no host, keeps UTF-16 paths", () => {
  expect(nativePath("C:\\Data")).toEqual(windowsPath("C:\\Data"));
});
