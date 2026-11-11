import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { RefObject } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { RecentScopePicker } from "../src/features/analysis/RecentScopePicker";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  RecentScopeList,
  RecentScopeOpen,
  ScopeDraft,
  ScopeReview,
} from "../src/ipc";

const propertyRestorers: (() => void)[] = [];

function mockProperty(
  target: object,
  name: PropertyKey,
  descriptor: PropertyDescriptor,
) {
  const original = Object.getOwnPropertyDescriptor(target, name);
  Object.defineProperty(target, name, descriptor);
  propertyRestorers.push(() => {
    if (original) Object.defineProperty(target, name, original);
    else Reflect.deleteProperty(target, name);
  });
}

function mockScrollTop(target: object, initialValue: number): number[] {
  const writes: number[] = [];
  let value = initialValue;
  mockProperty(target, "scrollTop", {
    configurable: true,
    get: () => value,
    set: (next: number) => {
      value = next;
      writes.push(next);
    },
  });
  return writes;
}

afterEach(() => {
  cleanup();
  document
    .querySelectorAll("[data-test-scroll-region]")
    .forEach((element) => element.remove());
  for (const restore of propertyRestorers.splice(0).reverse()) restore();
  vi.restoreAllMocks();
});

const when = "2026-11-10T00:00:00.000Z";
const locations = ["C:\\archive\\one", "D:\\archive\\two"];
const draft: ScopeDraft = {
  includes: locations.map(windowsPath),
  exclusions: [
    { kind: "path", path: windowsPath("C:\\archive\\one\\cache") },
    { kind: "name_pattern", pattern: "*.tmp" },
  ],
  boundary: "current_volume",
  includedMounts: [windowsPath("E:\\mount")],
  source: "recent",
};

function review(scope: ScopeDraft = draft, requestSeq = 70): ScopeReview {
  return {
    scope,
    notices: [],
    targets: scope.includes.map((path) => ({
      path,
      access: "accessible",
      reasonKey: null,
    })),
    reviewFingerprint: "review-fingerprint",
    observedAt: when,
    requestSeq,
  };
}

function opened(
  state: "matched" | "moved" | "disconnected" | "unverifiable" = "matched",
  currentReview: ScopeReview | null = review(),
  scope: ScopeDraft = draft,
): RecentScopeOpen {
  return {
    draft: scope,
    targets: scope.includes.map((path, index) => ({
      path,
      state: index === 0 ? state : "matched",
      proposedEntry:
        state === "moved" && index === 0 ? windowsPath("C:\\") : null,
    })),
    review: currentReview,
  };
}

function list(...refs: string[]): RecentScopeList {
  return {
    rows: refs.map((recentScopeRef) => ({
      recentScopeRef,
      includeCount: 2,
      lastUsedAt: when,
    })),
  };
}

function makeClient(
  handler: (name: string, body: unknown) => unknown | Promise<unknown>,
) {
  const calls: { name: string; body: unknown }[] = [];
  const client = {
    call: vi.fn(async (name: string, body: unknown) => {
      calls.push({ name, body });
      return await handler(name, body);
    }),
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  return { client, calls };
}

function picker(
  client: AnalysisClient,
  onApply = vi.fn(),
  scrollContainerRef?: RefObject<HTMLElement | null>,
  hasDraftEdits = false,
) {
  const sequences = [70, 71, 72];
  const onChoosePath = vi.fn();
  const currentSeq = { value: 70 };
  const nextRequestSeq = vi.fn(() => {
    const next = sequences.shift() ?? 73;
    currentSeq.value = next;
    return next;
  });
  const isRequestSeqCurrent = vi.fn(
    (requestSeq: number) => requestSeq === currentSeq.value,
  );
  const view = render(
    <RecentScopePicker
      client={client}
      hasDraftEdits={hasDraftEdits}
      nextRequestSeq={nextRequestSeq}
      isRequestSeqCurrent={isRequestSeqCurrent}
      onApply={onApply}
      onChoosePath={onChoosePath}
      {...(scrollContainerRef ? { scrollContainerRef } : {})}
    />,
  );
  return {
    onApply,
    nextRequestSeq,
    currentSeq,
    onChoosePath,
    rerender: () =>
      view.rerender(
        <RecentScopePicker
          client={client}
          hasDraftEdits={hasDraftEdits}
          nextRequestSeq={nextRequestSeq}
          isRequestSeqCurrent={isRequestSeqCurrent}
          onApply={onApply}
          onChoosePath={onChoosePath}
          {...(scrollContainerRef ? { scrollContainerRef } : {})}
        />,
      ),
  };
}

async function showList() {
  fireEvent.click(
    screen.getByRole("button", { name: "Recent analysis ranges" }),
  );
  await screen.findByRole("heading", { name: "Recent analysis ranges" });
}

test("F01 FR8: lists records in server order, opens only the selected reference, and applies reviewed conditions without starting a scan", async () => {
  const { client, calls } = makeClient(async (name, body) => {
    if (name === "scope.recent_list") return list("first", "second");
    if (name === "scope.recent_open")
      return opened(
        "matched",
        review(draft, (body as { requestSeq: number }).requestSeq),
      );
    throw new Error(`Unexpected command ${name}`);
  });
  const { onApply, nextRequestSeq } = picker(client);

  await showList();
  const buttons = screen.getAllByRole("button", { name: /Review range/ });
  expect(buttons).toHaveLength(2);
  expect(buttons[0]?.textContent).toBe("Review range 1");
  fireEvent.click(buttons[1]!);
  await screen.findByRole("heading", { name: "Review saved range" });
  expect(calls.filter(({ name }) => name === "scope.recent_open")).toEqual([
    {
      name: "scope.recent_open",
      body: { recentScopeRef: "second", requestSeq: 70 },
    },
  ]);
  expect(screen.getByText("C:\\archive\\one\\cache")).toBeDefined();
  expect(screen.getByText("*.tmp")).toBeDefined();
  expect(screen.getByText("Stay within each current volume")).toBeDefined();
  expect(screen.getByText("E:\\mount")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Use this range" }));
  expect(onApply).toHaveBeenCalledWith(draft, review());
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
  expect(nextRequestSeq).toHaveBeenCalledTimes(1);
});

test("F01 FR8: refuses apply when review is null or any target is unresolved", async () => {
  let response = opened("matched", null);
  let detailCalls = 0;
  const { client } = makeClient(async (name, body) => {
    if (name === "scope.recent_list") return list("one");
    detailCalls += 1;
    if (detailCalls === 1) return response;
    if (detailCalls === 2)
      return opened(
        "matched",
        review(draft, (body as { requestSeq: number }).requestSeq),
      );
    response = opened(
      "moved",
      review(draft, (body as { requestSeq: number }).requestSeq),
    );
    return response;
  });
  const { onApply } = picker(client);
  await showList();
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByRole("heading", { name: "Review saved range" });
  expect(
    screen.getByText(/Current paths and access could not be reviewed/),
  ).toBeDefined();
  expect(screen.queryByRole("button", { name: "Use this range" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Check paths again" }));
  await screen.findAllByText("Volume matched");
  expect(screen.getByRole("button", { name: "Use this range" })).toBeDefined();

  fireEvent.click(
    screen.getAllByRole("button", { name: "Back to recent ranges" })[0]!,
  );
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText("Location moved");
  expect(screen.getByText("Current volume entry:")).toBeDefined();
  expect(screen.getByText("C:\\")).toBeDefined();
  expect(
    screen.getByRole("button", { name: "Choose a path again" }),
  ).toBeDefined();
  expect(screen.queryByRole("button", { name: "Use this range" })).toBeNull();
  expect(onApply).not.toHaveBeenCalled();
});

test("F01 FR8: keeps focus on valid controls through replace confirmation and path recheck", async () => {
  const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView");
  let requestCount = 0;
  let finishRefresh!: (value: RecentScopeOpen) => void;
  const { client } = makeClient(async (name, body) => {
    if (name === "scope.recent_list") return list("one");
    if (name === "scope.recent_open") {
      requestCount += 1;
      if (requestCount === 3)
        return new Promise<RecentScopeOpen>((resolve) => {
          finishRefresh = resolve;
        });
      return opened(
        "matched",
        requestCount === 2
          ? null
          : review(draft, (body as { requestSeq: number }).requestSeq),
      );
    }
    throw new Error(`Unexpected command ${name}`);
  });
  picker(client, vi.fn(), undefined, true);
  await showList();
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByRole("button", { name: "Use this range" });

  fireEvent.click(screen.getByRole("button", { name: "Use this range" }));
  const replaceButton = screen.getByRole("button", {
    name: "Replace current scope",
    description: /Replace the current folder paths, exclusions, boundary/,
  });
  expect(
    screen.getByRole("button", {
      name: "Keep editing",
      description: /Replace the current folder paths, exclusions, boundary/,
    }),
  ).toBeDefined();
  await waitFor(() => expect(document.activeElement).toBe(replaceButton));
  expect(document.activeElement).not.toBe(document.body);
  expect(scrollIntoView.mock.contexts.at(-1)).toBe(replaceButton);
  expect(scrollIntoView).toHaveBeenLastCalledWith({
    block: "nearest",
    inline: "nearest",
  });

  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  const useButton = screen.getByRole("button", { name: "Use this range" });
  await waitFor(() => expect(document.activeElement).toBe(useButton));
  expect(document.activeElement).not.toBe(document.body);
  expect(scrollIntoView.mock.contexts.at(-1)).toBe(useButton);

  fireEvent.click(
    screen.getByRole("button", { name: "Back to recent ranges" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByRole("button", { name: "Choose a path again" });
  fireEvent.click(screen.getByRole("button", { name: "Check paths again" }));
  const detailHeading = screen.getByRole("heading", {
    name: "Review saved range",
  });
  await screen.findByText("Checking saved paths and access…");
  expect(document.activeElement).toBe(detailHeading.parentElement);
  expect(document.activeElement?.isConnected).toBe(true);
  await act(async () => finishRefresh(opened("matched", review(draft, 72))));
  await screen.findByRole("button", { name: "Use this range" });
  expect(document.activeElement).toBe(detailHeading.parentElement);
  expect(document.activeElement?.isConnected).toBe(true);
});

test.each([
  {
    name: "scrolls only enough to show the complete confirmation group",
    scrollMode: "editor",
    group: { top: 410, bottom: 470 },
    initialScrollTop: 100,
    expectedScrollTop: 172,
    expectedScrollDelta: 72,
    viewport: { top: 100, bottom: 400, height: 296, clientTop: 2 },
    expectedFocus: "button",
  },
  {
    name: "aligns the explanation when the confirmation group is taller than the viewport",
    scrollMode: "editor",
    group: { top: 40, bottom: 460 },
    initialScrollTop: 100,
    expectedScrollTop: 38,
    expectedScrollDelta: -62,
    viewport: { top: 100, bottom: 400, height: 296, clientTop: 2 },
    expectedFocus: "description",
  },
  {
    name: "uses the document viewport when the editor body is not scrollable",
    scrollMode: "document",
    group: { top: 778, bottom: 838 },
    initialScrollTop: 100,
    expectedScrollTop: 170,
    expectedScrollDelta: 70,
    viewport: { top: 0, bottom: 768, height: 768, clientTop: 0 },
    expectedFocus: "button",
  },
])(
  "F01 FR8: $name",
  async ({
    group,
    initialScrollTop,
    expectedScrollTop,
    expectedScrollDelta,
    scrollMode,
    viewport,
    expectedFocus,
  }) => {
    const { client } = makeClient(async (name, body) => {
      if (name === "scope.recent_list") return list("one");
      if (name === "scope.recent_open")
        return opened(
          "matched",
          review(draft, (body as { requestSeq: number }).requestSeq),
        );
      throw new Error(`Unexpected command ${name}`);
    });
    const editorBody = document.createElement("div");
    editorBody.className = "du-target-editor__body";
    editorBody.dataset.testScrollRegion = "true";
    editorBody.style.overflowY = scrollMode === "editor" ? "auto" : "hidden";
    document.body.append(editorBody);
    mockProperty(editorBody, "scrollHeight", {
      configurable: true,
      value: 1200,
    });
    mockProperty(editorBody, "clientHeight", {
      configurable: true,
      value: 296,
    });
    const editorScrollWrites = mockScrollTop(editorBody, initialScrollTop);
    if (scrollMode === "document") {
      const bodyOverflow = document.body.style.overflowY;
      const rootOverflow = document.documentElement.style.overflowY;
      propertyRestorers.push(() => {
        document.body.style.overflowY = bodyOverflow;
        document.documentElement.style.overflowY = rootOverflow;
      });
      document.body.style.overflowY = "visible";
      document.documentElement.style.overflowY = "auto";
    }
    const documentScroller =
      document.scrollingElement ?? document.documentElement;
    mockProperty(documentScroller, "scrollHeight", {
      configurable: true,
      value: 1200,
    });
    const documentScrollWrites = mockScrollTop(
      documentScroller,
      initialScrollTop,
    );
    const scrollContainer =
      scrollMode === "editor" ? editorBody : documentScroller;
    const scrollWrites =
      scrollMode === "editor" ? editorScrollWrites : documentScrollWrites;
    if (scrollMode === "document") {
      const nativeClientHeight = Object.getOwnPropertyDescriptor(
        HTMLElement.prototype,
        "clientHeight",
      )?.get;
      mockProperty(HTMLElement.prototype, "clientHeight", {
        configurable: true,
        get() {
          return this === document.documentElement
            ? viewport.height
            : (nativeClientHeight?.call(this) ?? 0);
        },
      });
    }
    const scrollContainerRef = { current: editorBody };
    picker(client, vi.fn(), scrollContainerRef, true);

    const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView");
    const bounds = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect");
    bounds.mockImplementation(function (this: HTMLElement) {
      if (this === scrollContainer)
        return {
          x: 0,
          y: viewport.top,
          top: viewport.top,
          right: 500,
          bottom: viewport.bottom,
          left: 0,
          width: 500,
          height: viewport.bottom - viewport.top,
          toJSON: () => ({}),
        };
      if (this.classList.contains("du-recent-scopes__confirm"))
        return {
          x: 0,
          y: group.top,
          top: group.top,
          right: 500,
          bottom: group.bottom,
          left: 0,
          width: 500,
          height: group.bottom - group.top,
          toJSON: () => ({}),
        };
      return {
        x: 0,
        y: 0,
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
        width: 0,
        height: 0,
        toJSON: () => ({}),
      };
    });
    if (scrollMode === "editor") {
      mockProperty(scrollContainer, "clientTop", {
        configurable: true,
        value: viewport.clientTop,
      });
      mockProperty(scrollContainer, "clientHeight", {
        configurable: true,
        value: viewport.height,
      });
    }

    await showList();
    fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
    await screen.findByRole("button", { name: "Use this range" });
    const beforeConfirm = scrollContainer.scrollTop;
    const writesBeforeConfirm = scrollWrites.length;
    if (scrollMode === "document") {
      const nativeFocus = HTMLElement.prototype.focus;
      vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (
        this: HTMLElement,
        options,
      ) {
        nativeFocus.call(this, options);
      });
    }
    fireEvent.click(screen.getByRole("button", { name: "Use this range" }));

    const groupElement = screen.getByRole("group", {
      name: "Replace current scope",
    });
    const description = groupElement.querySelector(
      ".du-recent-scopes__confirm-description",
    );
    const replaceButton = screen.getByRole("button", {
      name: "Replace current scope",
    });
    const keepEditing = screen.getByRole("button", { name: "Keep editing" });
    expect(description).not.toBeNull();
    expect(Array.from(groupElement.children)).toEqual([
      description,
      replaceButton,
      keepEditing,
    ]);
    expect(description?.getAttribute("tabindex")).toBe("-1");
    expect(replaceButton.tabIndex).toBe(0);
    expect(keepEditing.tabIndex).toBe(0);
    expect(scrollContainer.scrollTop).toBe(expectedScrollTop);
    expect(
      (scrollWrites.slice(writesBeforeConfirm).at(-1) ?? NaN) - beforeConfirm,
    ).toBe(expectedScrollDelta);
    expect(document.activeElement).toBe(
      expectedFocus === "description" ? description : replaceButton,
    );
    expect(scrollIntoView).not.toHaveBeenCalled();

    // happy-dom does not advance focus for Tab; DOM order and tabIndex cover the sequence here.
    // Native keyboard Tab traversal remains for integration verification.
    keepEditing.focus();
    fireEvent.keyDown(keepEditing, { key: " " });
    fireEvent.keyUp(keepEditing, { key: " " });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Use this range" }),
      ).toBeDefined(),
    );
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Use this range" }),
    );
  },
);

test("F01 FR8: renders an empty list and retries a failed list read", async () => {
  let fail = true;
  const { client, calls } = makeClient(async (name) => {
    if (name !== "scope.recent_list")
      throw new Error(`Unexpected command ${name}`);
    if (fail) throw new Error("storage unavailable");
    return list();
  });
  picker(client);
  await showList();
  await screen.findByRole("button", { name: "Try again" });
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText(/There are no recent analysis ranges/);
  expect(calls.filter(({ name }) => name === "scope.recent_list")).toHaveLength(
    2,
  );
});

test("F01 FR8: retries a failed detail read and names disconnected and unverifiable states", async () => {
  let fail = true;
  let state: "disconnected" | "unverifiable" = "disconnected";
  const { client, calls } = makeClient(async (name, body) => {
    if (name === "scope.recent_list") return list("one");
    if (name === "scope.recent_open") {
      if (fail) throw new Error("saved record damaged");
      return opened(
        state,
        review(draft, (body as { requestSeq: number }).requestSeq),
      );
    }
    throw new Error(`Unexpected command ${name}`);
  });
  picker(client);
  await showList();
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText(/saved range could not be opened/);
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText("Volume disconnected");
  expect(screen.queryByRole("button", { name: "Use this range" })).toBeNull();

  fireEvent.click(
    screen.getAllByRole("button", { name: "Back to recent ranges" })[0]!,
  );
  state = "unverifiable";
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText("Cannot verify that this is the same volume");
  expect(calls.filter(({ name }) => name === "scope.recent_open")).toHaveLength(
    3,
  );
  expect(
    screen.getByRole("button", { name: "Choose a path again" }),
  ).toBeDefined();
});

test("F01 FR8: ignores a late detail response after closing and reopening the list", async () => {
  let finishOpen!: (value: RecentScopeOpen) => void;
  const { client, calls } = makeClient((name) => {
    if (name === "scope.recent_list") return list("one");
    if (name === "scope.recent_open")
      return new Promise<RecentScopeOpen>((resolve) => {
        finishOpen = resolve;
      });
    throw new Error(`Unexpected command ${name}`);
  });
  const { onApply } = picker(client);
  await showList();
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText("Checking saved paths and access…");
  fireEvent.click(screen.getByRole("button", { name: "Close recent ranges" }));
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Recent analysis ranges" }),
    ),
  );
  await showList();
  await act(async () => finishOpen(opened()));
  expect(
    screen.queryByRole("heading", { name: "Review saved range" }),
  ).toBeNull();
  expect(onApply).not.toHaveBeenCalled();
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
});

test("F01 FR8: a newer selection wins when two detail requests finish out of order", async () => {
  let finishFirst!: (value: RecentScopeOpen) => void;
  const secondDraft: ScopeDraft = {
    ...draft,
    includes: [windowsPath("Z:\\newer-choice")],
  };
  const { client } = makeClient((name, body) => {
    if (name === "scope.recent_list") return list("first", "second");
    const requestSeq = (body as { requestSeq: number }).requestSeq;
    if ((body as { recentScopeRef: string }).recentScopeRef === "first")
      return new Promise<RecentScopeOpen>((resolve) => {
        finishFirst = resolve;
      });
    return opened("matched", review(secondDraft, requestSeq), secondDraft);
  });
  const { onApply } = picker(client);
  await showList();
  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText("Checking saved paths and access…");
  fireEvent.click(
    screen.getByRole("button", { name: "Back to recent ranges" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Review range 2" }));
  await screen.findAllByText("Z:\\newer-choice");
  await act(async () => finishFirst(opened("matched", review(draft, 70))));
  expect(screen.getAllByText("Z:\\newer-choice")).toHaveLength(2);
  expect(screen.queryByText("C:\\archive\\one")).toBeNull();
  expect(onApply).not.toHaveBeenCalled();
});

test("F01 FR8: rejects late successes and failures after the editor sequence changes, and rechecks before apply", async () => {
  let finishSuccess!: (value: RecentScopeOpen) => void;
  let finishFailure!: (error: Error) => void;
  const { client } = makeClient((name, body) => {
    if (name === "scope.recent_list")
      return list("success", "failure", "apply");
    const ref = (body as { recentScopeRef: string }).recentScopeRef;
    const requestSeq = (body as { requestSeq: number }).requestSeq;
    if (ref === "success")
      return new Promise<RecentScopeOpen>((resolve) => {
        finishSuccess = resolve;
      });
    if (ref === "failure")
      return new Promise<RecentScopeOpen>((_resolve, reject) => {
        finishFailure = reject;
      });
    return opened("matched", review(draft, requestSeq));
  });
  const { onApply, currentSeq, rerender } = picker(client);
  await showList();

  fireEvent.click(screen.getByRole("button", { name: "Review range 1" }));
  await screen.findByText("Checking saved paths and access…");
  const focusedDetailHeading = document.activeElement;
  currentSeq.value = 71;
  await act(async () => finishSuccess(opened("matched", review(draft, 70))));
  await screen.findByText("The scope changed. Check the saved range again.");
  expect(screen.queryByRole("button", { name: "Use this range" })).toBeNull();
  expect(document.activeElement).toBe(focusedDetailHeading);
  expect(onApply).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Try again" })).toBeDefined();
  fireEvent.click(
    screen.getAllByRole("button", { name: "Back to recent ranges" })[0]!,
  );

  fireEvent.click(screen.getByRole("button", { name: "Review range 2" }));
  await screen.findByText("Checking saved paths and access…");
  currentSeq.value = 72;
  await act(async () => finishFailure(new Error("review transport failed")));
  await screen.findByText("The scope changed. Check the saved range again.");
  expect(screen.queryByRole("button", { name: "Use this range" })).toBeNull();
  expect(onApply).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getAllByRole("button", { name: "Back to recent ranges" })[0]!,
  );

  fireEvent.click(screen.getByRole("button", { name: "Review range 3" }));
  await screen.findByRole("button", { name: "Use this range" });
  const applyBeforeEdit = screen.getByRole("button", {
    name: "Use this range",
  });
  currentSeq.value = 73;
  fireEvent.click(applyBeforeEdit);
  expect(onApply).not.toHaveBeenCalled();
  rerender();
  const staleApply = screen.queryByRole("button", { name: "Use this range" });
  expect(staleApply).toBeNull();
  expect(screen.getByText(/editor scope changed/)).toBeDefined();
  expect(onApply).not.toHaveBeenCalled();
  expect(
    screen.getByRole("button", { name: "Check paths again" }),
  ).toBeDefined();
});

test("F01 FR8: preserves long paths and returns focus to the originating row", async () => {
  const longPath = `C:\\archive\\${"very-long-folder-name\\".repeat(18)}final`;
  const longDraft = { ...draft, includes: [windowsPath(longPath)] };
  const { client } = makeClient(async (name, body) => {
    if (name === "scope.recent_list") return list("one");
    if (name === "scope.recent_open")
      return opened(
        "matched",
        review(longDraft, (body as { requestSeq: number }).requestSeq),
        longDraft,
      );
    throw new Error(`Unexpected command ${name}`);
  });
  const scrollContainerRef = { current: document.createElement("div") };
  scrollContainerRef.current.scrollTop = 225;
  picker(client, vi.fn(), scrollContainerRef);
  await showList();
  scrollContainerRef.current.scrollTop = 125;
  const row = screen.getByRole("button", { name: "Review range 1" });
  fireEvent.click(row);
  await screen.findAllByText(longPath);
  scrollContainerRef.current.scrollTop = 0;
  fireEvent.click(
    screen.getByRole("button", { name: "Back to recent ranges" }),
  );
  expect(scrollContainerRef.current.scrollTop).toBe(125);
  await waitFor(() =>
    expect(document.activeElement?.textContent).toContain("Review range 1"),
  );
  expect(
    screen.getByRole("heading", { name: "Recent analysis ranges" }),
  ).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Close recent ranges" }));
  expect(scrollContainerRef.current.scrollTop).toBe(225);
});
