import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { Gallery } from "../preview/Gallery";

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.theme;
});

/** The value of the summary's exclusions row. */
function exclusions() {
  return screen
    .getByText("제외 규칙", { selector: '[role="note"]' })
    .closest("dt")!.nextElementSibling!;
}

test("the gallery shows the product's result summary with each state of the exclusions row", () => {
  render(<Gallery />);
  fireEvent.change(screen.getByLabelText("검토할 화면"), {
    target: { value: "summary" },
  });
  expect(exclusions().textContent).toBe(
    "C:\\Users\\민서\\문서\\보관함, C:\\Users\\민서\\문서\\임시 파일",
  );

  fireEvent.change(screen.getByLabelText("제외 규칙 상태"), {
    target: { value: "five" },
  });
  const toggle = screen.getByRole("button", { name: "제외 규칙 외 2개" });
  fireEvent.click(toggle);
  expect(exclusions().textContent).toContain("*.tmp");
  expect(toggle.getAttribute("aria-label")).toBe("제외 규칙 접기");

  fireEvent.change(screen.getByLabelText("제외 규칙 상태"), {
    target: { value: "unknown" },
  });
  expect(exclusions().textContent).toBe("확인 불가");
});
