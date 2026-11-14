import axe from "axe-core";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { InfoEntry } from "../src/app/InfoEntry";
import { LocaleTextContext } from "../src/i18n/presentation";
import { fakeHost, productInfo, supportReport } from "./info-host";

afterEach(cleanup);

const host = () =>
  fakeHost({
    "app.product_info": () => productInfo(),
    "app.support_info": () => supportReport(),
  });

test("reaches both screens with one click each and returns focus to the opener", async () => {
  const { client } = host();
  const { container } = render(<InfoEntry client={client} />);
  expect((await axe.run(container)).violations).toEqual([]);
  const nav = screen.getByRole("navigation", { name: "Product and support" });
  expect(nav).toBeTruthy();

  const about = screen.getByRole("button", { name: "About DiskUnfold" });
  about.focus();
  fireEvent.click(about);
  const dialog = await screen.findByRole("dialog", {
    name: "About DiskUnfold",
  });
  expect(await screen.findByText("0.0.1")).toBeTruthy();
  expect((await axe.run(dialog)).violations).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Close details" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(document.activeElement).toBe(about));

  const support = screen.getByRole("button", { name: "Support information" });
  fireEvent.click(support);
  const second = await screen.findByRole("dialog", {
    name: "Support information",
  });
  expect(
    (await screen.findAllByText("Officially verified")).length,
  ).toBeGreaterThan(0);
  expect((await axe.run(second)).violations).toEqual([]);
});

test("renders nothing without a host connection and uses the fallback connection when given", () => {
  const { client } = host();
  const none = render(<InfoEntry client={undefined} />);
  expect(none.container.textContent).toBe("");
  none.unmount();
  render(<InfoEntry client={undefined} fallbackClient={client} />);
  expect(
    screen.getByRole("button", { name: "Support information" }),
  ).toBeTruthy();
});

test("uses the shared locale text for labels", () => {
  const { client } = host();
  render(
    <LocaleTextContext.Provider
      value={(key, fallback) =>
        key === "ui.info.product_open" ? "DiskUnfold 정보" : fallback
      }
    >
      <InfoEntry client={client} standalone />
    </LocaleTextContext.Provider>,
  );
  expect(screen.getByRole("button", { name: "DiskUnfold 정보" })).toBeTruthy();
});

test("keeps the opener buttons in reading order and puts focus on the close control inside the dialog", async () => {
  const { client } = host();
  render(<InfoEntry client={client} />);
  const nav = screen.getByRole("navigation", { name: "Product and support" });
  const buttons = Array.from(nav.querySelectorAll("button"));
  expect(buttons.map((button) => button.textContent)).toEqual([
    "About DiskUnfold",
    "Support information",
  ]);
  expect(buttons.every((button) => !(button.tabIndex > 0))).toBe(true);

  fireEvent.click(buttons[0] as HTMLElement);
  const dialog = await screen.findByRole("dialog", {
    name: "About DiskUnfold",
  });
  await screen.findByText("0.0.1");
  const close = screen.getByRole("button", { name: "Close details" });
  await waitFor(() => expect(document.activeElement).toBe(close));
  const inside = Array.from(dialog.querySelectorAll("button"));
  expect(inside[0]).toBe(close);
  expect(inside.map((button) => button.textContent)).toContain("Refresh");
  expect(inside.every((button) => !(button.tabIndex > 0))).toBe(true);
});

test.each([
  ["About DiskUnfold", "About DiskUnfold"],
  ["Support information", "Support information"],
])(
  "Escape closes the %s dialog and returns focus to its opener",
  async (opener, title) => {
    const { client } = host();
    render(<InfoEntry client={client} />);
    const button = screen.getByRole("button", { name: opener });
    button.focus();
    fireEvent.click(button);
    const dialog = await screen.findByRole("dialog", { name: title });
    fireEvent.keyDown(dialog, { key: "Escape", code: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(button));
    expect(
      screen.getByRole("navigation", { name: "Product and support" }),
    ).toBeTruthy();
  },
);
