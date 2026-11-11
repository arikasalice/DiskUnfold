import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Gallery } from "../preview/Gallery";
import coverage from "../preview/feature-coverage.json";
import { fixtures } from "../preview/surface-fixtures";

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.theme;
});

test("every AIDA feature has one accountable primary surface and explicit integration status", () => {
  expect(coverage.map((item) => item.feature).sort()).toEqual(
    Array.from(
      { length: 45 },
      (_, index) => `F${String(index + 1).padStart(2, "0")}`,
    ),
  );
  for (const item of coverage) {
    expect(Object.hasOwn(fixtures, item.primarySurface)).toBe(true);
    expect(["existing-partial-ui", "presentation-only"]).toContain(
      item.delivery,
    );
    expect(item.integrationNote.length).toBeGreaterThan(0);
  }
});

test("gallery recovery is explicit and inspection returns to its originating control", async () => {
  render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "duplicates" },
  });
  const opener = screen.getByRole("button", {
    name: "휴가 영상 묶음",
  });
  act(() => opener.focus());
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog", { name: "휴가 영상 묶음" });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Close details" }),
  );
  await act(async () => {});
  await waitFor(() => expect(document.activeElement).toBe(opener));
  fireEvent.change(screen.getByLabelText("화면 상태"), {
    target: { value: "error" },
  });
  expect(screen.queryByRole("button", { name: "휴가 영상 묶음" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "샘플 상태 복구" }));
  expect(screen.getByRole("button", { name: "휴가 영상 묶음" })).toBeDefined();
});

test("right click exposes the same menu and unavailable file actions stay disabled", async () => {
  render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "duplicates" },
  });
  fireEvent.contextMenu(screen.getByRole("button", { name: "휴가 영상 묶음" }));
  const menu = await screen.findByRole("menu", { name: "휴가 영상 묶음 동작" });
  expect(
    within(menu)
      .getByRole("menuitem", { name: /파일 위치 열기/ })
      .getAttribute("aria-disabled"),
  ).toBe("true");
  expect(
    within(menu).getByRole("menuitem", { name: "자세히 보기" }),
  ).toBeDefined();
});

test("approval preview never enables execution when its confirmation is checked", () => {
  render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "approval" },
  });
  fireEvent.click(screen.getByRole("checkbox"));
  expect(
    (
      screen.getByRole("button", {
        name: "승인 불가 — 실제 요청 없음",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});

test("selecting a graph shape preserves the workspace until details are requested", async () => {
  const { container } = render(<Gallery />);
  fireEvent.click(container.querySelector(".du-geometry-shape rect")!);
  expect(screen.queryByRole("dialog")).toBeNull();
  const details = screen.getByRole("button", { name: "선택 항목 자세히 보기" });
  act(() => details.focus());
  fireEvent.click(details);
  const dialog = await screen.findByRole("dialog", { name: "영상" });
  expect(within(dialog).getByText("48 GiB")).toBeDefined();
});
