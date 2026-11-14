import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "../src/app/App";
import { TitleBarProvider } from "../src/components";
import { createAnalysisClient } from "../src/ipc";

afterEach(cleanup);

/** A client whose every command fails: the screen label does not depend on any reply. */
const client = createAnalysisClient(async () => {
  throw new Error("offline");
});

test("the main workspace names its screen in the window title", async () => {
  render(
    <TitleBarProvider>
      <App analysis={client} />
    </TitleBarProvider>,
  );
  await waitFor(() =>
    expect(document.title).toBe("DiskUnfold \u2014 Analysis"),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Operation history" }),
  );
  await waitFor(() =>
    expect(document.title).toBe("DiskUnfold \u2014 Operation history"),
  );
});

test("without a desktop connection the title names only the app", async () => {
  render(
    <TitleBarProvider>
      <App />
    </TitleBarProvider>,
  );
  await waitFor(() => expect(document.title).toBe("DiskUnfold"));
});
