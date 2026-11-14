import axe from "axe-core";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { NoticeViewer } from "../src/features/product/NoticeViewer";
import { productCopy } from "../src/features/product/product-copy";
import { ProductScreen } from "../src/features/product/ProductScreen";
import { HostFailure, SHA, fakeHost, productInfo } from "./info-host";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const press = (name: string | RegExp) =>
  fireEvent.click(screen.getByRole("button", { name }));

test("shows the build facts, opens a notice on demand and has no accessibility violations", async () => {
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.notices": (body) => ({
      kind: body.kind,
      text: "MIT License text",
      sourceCommit: "6cd1a30c9f",
    }),
  });
  const { container } = render(<ProductScreen client={host.client} />);
  expect(await screen.findByText("0.0.1")).toBeTruthy();
  const facts = within(container.querySelector("dl") as HTMLElement);
  expect(facts.getByText("Official build")).toBeTruthy();
  expect(facts.getByText("6cd1a30c9f")).toBeTruthy();
  expect(facts.getByText("Windows installer (MSI)")).toBeTruthy();
  expect(facts.getByText("sm-7")).toBeTruthy();
  expect(facts.getByText("2026-11-01")).toBeTruthy();
  expect(host.count("app.notices")).toBe(0);

  press(/License/);
  expect(await screen.findByText("MIT License text")).toBeTruthy();
  expect(host.calls.find((c) => c.name === "app.notices")?.body).toEqual({
    kind: "LICENSE",
  });
  expect(
    screen.getByText(/Generated from source commit: 6cd1a30c9f/),
  ).toBeTruthy();
  expect(
    screen.getByRole("region", { name: "Document text: License" }),
  ).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("calculates the digest, offers a copy and never calls it verified", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": (body) => ({
      correlationId: body.correlationId,
      sha256: SHA,
      reasonKey: null,
    }),
  });
  const { container } = render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  expect(await screen.findByText(SHA)).toBeTruthy();
  expect(container.textContent).toContain(
    "does not treat this value as verified",
  );
  expect(container.textContent).not.toMatch(/verified successfully|✓/i);
  press("Copy digest");
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(SHA));
  expect(await screen.findByText("Digest copied.")).toBeTruthy();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("reports a failed copy without hiding the digest", async () => {
  vi.stubGlobal("navigator", {});
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": (body) => ({
      correlationId: body.correlationId,
      sha256: SHA,
      reasonKey: null,
    }),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  await screen.findByText(SHA);
  press("Copy digest");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not be copied",
  );
  expect(screen.getByText(SHA)).toBeTruthy();
});

test("states every unavailable or unknown value with a reason", async () => {
  const host = fakeHost({
    "app.product_info": () =>
      productInfo({
        productVersion: null,
        buildKind: "unknown",
        commit: null,
        builtAtUtc: null,
        targetOs: null,
        targetCpu: null,
        installForm: "unknown",
        installFormReasonKey: "product.install_form_unknown",
        appDbFormat: null,
        snapshotFormat: null,
        supportMatrixRevision: null,
        supportMatrixAsOf: null,
        notices: [
          { kind: "LICENSE", available: false },
          { kind: "THIRD-PARTY-NOTICES", available: false },
        ],
      }),
    "app.executable_digest": (body) => ({
      correlationId: body.correlationId,
      sha256: null,
      reasonKey: "product.digest_unavailable",
    }),
  });
  const { container } = render(<ProductScreen client={host.client} />);
  expect(await screen.findAllByText("Not recorded in this build")).toHaveLength(
    8,
  );
  expect(
    screen.getByText(
      /could not tell whether this is an official or a developer/,
    ),
  ).toBeTruthy();
  expect(screen.getByText(/could not tell how it was installed/)).toBeTruthy();
  expect(screen.getAllByText(/Not included in this build\./)).toHaveLength(2);
  expect(screen.queryByRole("button", { name: /License/ })).toBeNull();
  press("Calculate digest");
  expect((await screen.findByRole("status")).textContent).toContain(
    "could not read its own executable file",
  );
  expect(screen.queryByText("Copy digest")).toBeNull();
  expect((await axe.run(container)).violations).toEqual([]);
});

test("marks a developer build and survives a notice that has no text", async () => {
  const host = fakeHost({
    "app.product_info": () => productInfo({ buildKind: "developer" }),
    "app.notices": (body) => ({
      kind: body.kind,
      text: null,
      sourceCommit: null,
    }),
  });
  render(<ProductScreen client={host.client} />);
  expect(await screen.findByText("Developer build")).toBeTruthy();
  expect(screen.getByText(/not an official release/)).toBeTruthy();
  press(/Third-party notices/);
  expect(await screen.findByText(/does not include the text/)).toBeTruthy();
});

test("retries a failed product read and a failed notice read", async () => {
  let reads = 0;
  let noticeReads = 0;
  const host = fakeHost({
    "app.product_info": () =>
      ++reads === 1 ? new HostFailure("support.internal_error") : productInfo(),
    "app.notices": (body) =>
      ++noticeReads === 1
        ? new HostFailure()
        : { kind: body.kind, text: "Licence body", sourceCommit: null },
  });
  render(<ProductScreen client={host.client} />);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Product information could not be read.",
  );
  press("Retry");
  await screen.findByText("0.0.1");
  expect(screen.queryByRole("alert")).toBeNull();

  press(/License/);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "The document could not be read.",
  );
  press("Retry");
  expect(await screen.findByText("Licence body")).toBeTruthy();
  expect(
    screen.getByText("The source commit of this document is not recorded."),
  ).toBeTruthy();
});

test("keeps the last values when a refresh fails", async () => {
  let reads = 0;
  const host = fakeHost({
    "app.product_info": () =>
      ++reads === 1 ? productInfo() : new HostFailure(),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Refresh");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "last successful read",
  );
  expect(screen.getByText("0.0.1")).toBeTruthy();
});

test("drops a digest response that arrives after the screen closed", async () => {
  const pending: ((response: unknown) => void)[] = [];
  const ids: string[] = [];
  const late = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": (body) =>
      new Promise((resolve) => {
        ids.push(String(body.correlationId));
        pending.push(resolve);
      }),
  });
  const { unmount } = render(<ProductScreen client={late.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  await waitFor(() => expect(ids).toHaveLength(1));
  unmount();
  await act(async () =>
    pending[0]?.({
      correlationId: ids[0],
      sha256: "cd".repeat(32),
      reasonKey: null,
    }),
  );
  expect(document.body.textContent).not.toContain("cdcd");
});

test("treats a digest response that echoes another request as a failure instead of waiting", async () => {
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": () => ({
      correlationId: "someone-else",
      sha256: SHA,
      reasonKey: null,
    }),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  await waitFor(() => expect(host.count("app.executable_digest")).toBe(1));
  await act(async () => undefined);
  expect(screen.queryByText(SHA)).toBeNull();
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not be requested",
  );
});

test.each([
  ["too short", "ab".repeat(31)],
  ["too long", "ab".repeat(33)],
  ["not hexadecimal", "zz".repeat(32)],
  ["markup", "<b>" + "a".repeat(61)],
  ["empty", ""],
])("rejects a digest that is %s", async (_name, value) => {
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": (body) => ({
      correlationId: body.correlationId,
      sha256: value,
      reasonKey: null,
    }),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not be requested",
  );
  expect(screen.queryByText("Copy digest")).toBeNull();
  if (value) expect(document.body.textContent).not.toContain(value);
});

test("accepts an uppercase hexadecimal digest", async () => {
  const upper = "AB".repeat(32);
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": (body) => ({
      correlationId: body.correlationId,
      sha256: upper,
      reasonKey: null,
    }),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  expect(await screen.findByText(upper)).toBeTruthy();
});

function recordingText(overrides: Record<string, string> = {}) {
  const calls: { key: string; values?: Record<string, unknown> }[] = [];
  const text: LocaleText = (key, fallback, values) => {
    calls.push(values ? { key, values } : { key });
    return interpolate(overrides[key] ?? fallback, (name) =>
      String(values?.[name] ?? ""),
    );
  };
  return { calls, text };
}

test("opens with the introduction and names the build target like the support screen", async () => {
  const { calls, text } = recordingText({
    "ui.support.os_windows": "윈도우",
  });
  const host = fakeHost({ "app.product_info": () => productInfo() });
  const { container } = render(
    <LocaleTextContext.Provider value={text}>
      <ProductScreen client={host.client} />
    </LocaleTextContext.Provider>,
  );
  await screen.findByText("0.0.1");
  const first = container.querySelector(".du-info-screen")?.firstElementChild;
  expect(first?.textContent).toContain("does not contact any server");
  expect(
    within(container.querySelector("dl") as HTMLElement).getByText(
      "윈도우 / x64",
    ),
  ).toBeTruthy();
  expect(calls).toContainEqual({
    key: "ui.product.target_value",
    values: { os: "윈도우", cpu: "x64" },
  });
});

test("names a target the host reported in words this version lacks as reported", async () => {
  const host = fakeHost({
    "app.product_info": () =>
      productInfo({ targetOs: "haiku", targetCpu: null }),
  });
  const { container } = render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  expect(
    within(container.querySelector("dl") as HTMLElement).getByText(
      "haiku / Not recorded in this build",
    ),
  ).toBeTruthy();
});

test("labels carry their value through one locale message", async () => {
  const { calls, text } = recordingText();
  const host = fakeHost({
    "app.product_info": () =>
      productInfo({
        notices: [
          { kind: "LICENSE", available: true },
          { kind: "THIRD-PARTY-NOTICES", available: false },
        ],
      }),
    "app.notices": (body) => ({
      kind: body.kind,
      text: "Body",
      sourceCommit: "abc123",
    }),
  });
  render(
    <LocaleTextContext.Provider value={text}>
      <ProductScreen client={host.client} />
    </LocaleTextContext.Provider>,
  );
  await screen.findByText("0.0.1");
  press(/License/);
  await screen.findByText("Body");
  expect(screen.getByText("Generated from source commit: abc123")).toBeTruthy();
  expect(
    screen.getByRole("region", { name: "Document text: License" }),
  ).toBeTruthy();
  expect(
    screen.getByText("Third-party notices: Not included in this build."),
  ).toBeTruthy();
  expect(calls).toContainEqual({
    key: "ui.product.notice_commit_value",
    values: { commit: "abc123" },
  });
  expect(calls).toContainEqual({
    key: "ui.product.notice_region_named",
    values: { name: "License" },
  });
  expect(calls).toContainEqual({
    key: "ui.product.notice_missing_named",
    values: { name: "Third-party notices" },
  });
});

const baselineText: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));

test("a notice body never outlives the document it was read for", async () => {
  const pending = new Map<string, (value: unknown) => void>();
  const host = fakeHost({
    "app.notices": (body) =>
      new Promise((resolve) => {
        pending.set(String(body.kind), resolve);
      }),
  });
  const copy = productCopy(baselineText);
  const view = render(
    <NoticeViewer client={host.client} kind="LICENSE" available copy={copy} />,
  );
  press(/License/);
  await waitFor(() => expect(pending.has("LICENSE")).toBe(true));
  await act(async () =>
    pending.get("LICENSE")?.({
      kind: "LICENSE",
      text: "License body",
      sourceCommit: null,
    }),
  );
  expect(await screen.findByText("License body")).toBeTruthy();

  view.rerender(
    <NoticeViewer
      client={host.client}
      kind="THIRD-PARTY-NOTICES"
      available
      copy={copy}
    />,
  );
  expect(screen.queryByText("License body")).toBeNull();
  expect(screen.getByRole("progressbar")).toBeTruthy();

  await waitFor(() => expect(pending.has("THIRD-PARTY-NOTICES")).toBe(true));
  await act(async () =>
    pending.get("THIRD-PARTY-NOTICES")?.({
      kind: "THIRD-PARTY-NOTICES",
      text: "Third body",
      sourceCommit: null,
    }),
  );
  expect(await screen.findByText("Third body")).toBeTruthy();
  expect(screen.queryByText("License body")).toBeNull();
});

test("a late reply for the previous document is ignored", async () => {
  const pending = new Map<string, (value: unknown) => void>();
  const host = fakeHost({
    "app.notices": (body) =>
      new Promise((resolve) => {
        pending.set(String(body.kind), resolve);
      }),
  });
  const copy = productCopy(baselineText);
  const view = render(
    <NoticeViewer client={host.client} kind="LICENSE" available copy={copy} />,
  );
  press(/License/);
  await waitFor(() => expect(pending.has("LICENSE")).toBe(true));
  view.rerender(
    <NoticeViewer
      client={host.client}
      kind="THIRD-PARTY-NOTICES"
      available
      copy={copy}
    />,
  );
  await waitFor(() => expect(pending.has("THIRD-PARTY-NOTICES")).toBe(true));
  await act(async () =>
    pending.get("LICENSE")?.({
      kind: "LICENSE",
      text: "Late license body",
      sourceCommit: null,
    }),
  );
  expect(screen.queryByText("Late license body")).toBeNull();
  expect(screen.getByRole("progressbar")).toBeTruthy();
});

test("reports a failed digest request", async () => {
  const host = fakeHost({
    "app.product_info": () => productInfo(),
    "app.executable_digest": () => new HostFailure(),
  });
  render(<ProductScreen client={host.client} />);
  await screen.findByText("0.0.1");
  press("Calculate digest");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not be requested",
  );
});
