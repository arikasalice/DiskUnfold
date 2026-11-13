import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "../src/app/App";

afterEach(cleanup);

test("renders an accessible foundation shell", () => {
  render(<App />);
  const heading = screen.getByRole("heading", { level: 1, name: "DiskUnfold" });
  expect(screen.getByRole("main").contains(heading)).toBe(true);
});
