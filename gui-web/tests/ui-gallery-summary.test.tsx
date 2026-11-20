import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Gallery } from "../preview/Gallery";
import { historyLocations } from "../preview/history-items-fixture";

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.theme;
});

/** The rules the summary's exclusions row shows, one text for each line. */
function exclusions() {
  const value = screen
    .getByText("제외 규칙", { selector: '[role="note"]' })
    .closest("dt")!.nextElementSibling!;
  return Array.from(
    value.querySelectorAll('[role="listitem"]'),
    (rule) => rule.textContent,
  );
}

test("the gallery shows the product's result summary with each state of the exclusions row", () => {
  render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "summary" },
  });
  expect(exclusions()).toEqual([
    "C:\\Users\\민서\\문서\\보관함",
    "C:\\Users\\민서\\문서\\임시 파일, 2026",
  ]);

  fireEvent.change(screen.getByLabelText("제외 규칙 상태"), {
    target: { value: "five" },
  });
  expect(exclusions()[1]).toBe("이름 패턴 *.tmp");
  const toggle = screen.getByRole("button", { name: "제외 규칙 2개 더 보기" });
  fireEvent.click(toggle);
  expect(exclusions()[4]).toBe("이름 패턴 node_modules");
  expect(toggle.getAttribute("aria-label")).toBe("제외 규칙 접기");

  fireEvent.change(screen.getByLabelText("제외 규칙 상태"), {
    target: { value: "unknown" },
  });
  expect(exclusions()).toEqual([]);
  expect(screen.getByText("확인 불가")).toBeDefined();
});

test("the gallery draws the history item results with the product's table and a value outside it", async () => {
  const { container } = render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "history" },
  });
  await screen.findByText("기록된 위치");
  const heard = Array.from(
    container.querySelectorAll(
      ".du-history-item-location .du-fit > .du-visually-hidden",
    ),
    (copy) => copy.textContent,
  );
  expect(heard.slice(0, 2)).toEqual([
    historyLocations.short,
    historyLocations.long,
  ]);
  expect(heard).toHaveLength(4);
  expect(
    screen
      .getByRole("region", { name: "표 밖 단독 값" })
      .querySelector(".du-middle"),
  ).not.toBeNull();
});
