import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import { AnalysisError } from "../src/ipc";
import {
  deferred,
  sampleClient,
  sampleEvent,
  sampleSession,
} from "./sample-support";

const panel = vi.hoisted(() => ({ mounts: 0 }));
vi.mock("../src/features/analysis/AnalysisPanel", async () => {
  const { useEffect, useState } = await import("react");
  return {
    AnalysisPanel: ({ startAlternative }: { startAlternative?: unknown }) => {
      const [draft, setDraft] = useState("");
      useEffect(() => {
        panel.mounts += 1;
      }, []);
      return (
        <div>
          <input
            aria-label="Draft path"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
          />
          {startAlternative as never}
        </div>
      );
    },
  };
});
vi.mock("../src/features/settings/SettingsRestoration", () => ({
  SettingsRestoration: () => null,
}));
vi.mock("../src/features/browse/BrowsePanel", () => ({
  BrowsePanel: (props: { resultVersion?: string; rootLabel?: string }) => (
    <div
      data-testid="sample-browse"
      data-version={props.resultVersion}
      data-root={props.rootLabel}
    />
  ),
}));

beforeEach(() => {
  panel.mounts = 0;
});
afterEach(cleanup);

const button = (name: string) => screen.getByRole("button", { name });

async function confirmDialog(name: string) {
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain(
    "Only the sample experience state is discarded.",
  );
  fireEvent.click(within(dialog).getByRole("button", { name }));
}

function openedFixture(session = sampleSession(), extra = {}) {
  return sampleClient({
    "sample.open": () => session,
    "sample.status": () => session,
    "sample.reset": () => ({ ...session, sequence: session.sequence + 1 }),
    "sample.close": () => ({
      sessionRef: session.sessionRef,
      state: "closed",
      releasedVersions: 2,
    }),
    ...extra,
  });
}

async function start(fake: ReturnType<typeof sampleClient>) {
  render(<App analysis={fake.client} />);
  fireEvent.click(button("Try sample data first"));
  await screen.findByRole("region", { name: "Sample experience" });
}

test("the sample strip is always visible with words and an icon and names the bundle version", async () => {
  await start(openedFixture());
  const strip = screen.getByRole("region", { name: "Sample experience" });
  expect(strip.textContent).toContain("Sample experience in progress");
  expect(strip.textContent).toContain("Sample set version 2026.1");
  expect(strip.querySelector("svg")).not.toBeNull();
  expect(
    within(strip)
      .getAllByRole("button")
      .map((b) => b.textContent),
  ).toEqual(["End experience", "Start real analysis"]);
});

test("an active session browses its latest observation under the sample root and can switch time", async () => {
  await start(openedFixture());
  let browse = await screen.findByTestId("sample-browse");
  expect(browse.dataset.version).toBe("rv1");
  expect(browse.dataset.root).toBe("Sample data");
  fireEvent.change(screen.getByLabelText("Observation time"), {
    target: { value: "0" },
  });
  browse = screen.getByTestId("sample-browse");
  expect(browse.dataset.version).toBe("rv0");
});

test("starting while a request is pending sends one command and disables the entry", async () => {
  const gate = deferred<ReturnType<typeof sampleSession>>();
  const fake = openedFixture(sampleSession(), {
    "sample.open": () => gate.promise,
  });
  render(<App analysis={fake.client} />);
  const entry = button("Try sample data first");
  fireEvent.click(entry);
  await waitFor(() => expect((entry as HTMLButtonElement).disabled).toBe(true));
  fireEvent.click(entry);
  expect(fake.count("sample.open")).toBe(1);
  await act(async () => gate.resolve(sampleSession()));
  await screen.findByRole("region", { name: "Sample experience" });
});

test("a busy start asks the user to try again", async () => {
  const fake = openedFixture(sampleSession(), {
    "sample.open": () => {
      throw new AnalysisError("BUSY", "job.queue_full", true);
    },
  });
  render(<App analysis={fake.client} />);
  fireEvent.click(button("Try sample data first"));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Try again in a moment",
  );
  expect(
    screen.queryByRole("region", { name: "Sample experience" }),
  ).toBeNull();
});

test("preparing and resetting show progress and the end action stays available", async () => {
  const fake = openedFixture(sampleSession({ state: "preparing" }));
  await start(fake);
  expect(
    screen.getByRole("progressbar", { name: "Preparing the sample data" }),
  ).toBeTruthy();
  expect(button("End experience")).toBeTruthy();
  fake.emit(
    sampleEvent("sample.state_changed", {
      sessionRef: "sample-1",
      state: "loading",
      sequence: 2,
    }),
  );
  expect(
    screen.getByRole("progressbar", { name: "Preparing the sample data" }),
  ).toBeTruthy();
});

test.each([
  ["sample.missing", "missing from this installation"],
  ["sample.format_unsupported", "cannot read the sample data format"],
  ["sample.bundle_invalid", "damaged"],
  ["sample.load_failed", "could not be loaded"],
  ["sample.something_new", "cannot be used right now"],
  [undefined, "cannot be used right now"],
])(
  "an unavailable sample explains %s and offers only retry and real analysis",
  async (reason, wording) => {
    const fake = openedFixture(
      sampleSession({
        state: "unavailable",
        observations: [],
        ...(reason ? { unavailableReason: reason } : {}),
      }),
    );
    await start(fake);
    const notice = screen.getByRole("region", {
      name: "The sample experience is unavailable",
    });
    expect(notice.textContent).toContain(wording);
    expect(notice.textContent).not.toContain("sample.");
    expect(
      within(notice)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Try again", "Start real analysis"]);
    // The strip offers no further actions, so these are the only two on screen.
    expect(
      screen.getAllByRole("button").map((b) => b.textContent),
    ).not.toContain("End experience");

    fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
    await confirmDialog("Reset experience");
    await waitFor(() => expect(fake.count("sample.reset")).toBe(1));
  },
);

test("cancelling a confirmation returns the focus to the control that asked", async () => {
  const fake = openedFixture();
  await start(fake);
  const opener = button("End experience");
  act(() => opener.focus());
  fireEvent.click(opener);
  const dialog = await screen.findByRole("alertdialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(document.activeElement).toBe(opener));
  expect(fake.count("sample.close")).toBe(0);
});

test("switching to real analysis remounts the target screen only after close answered", async () => {
  const gate = deferred<unknown>();
  const fake = openedFixture(sampleSession(), {
    "sample.close": () => gate.promise,
  });
  render(<App analysis={fake.client} />);
  expect(panel.mounts).toBe(1);
  fireEvent.change(screen.getByLabelText("Draft path"), {
    target: { value: "D:\\old selection" },
  });
  fireEvent.click(button("Try sample data first"));
  await screen.findByRole("region", { name: "Sample experience" });

  fireEvent.click(button("Start real analysis"));
  await confirmDialog("Start real analysis");
  await waitFor(() => expect(fake.count("sample.close")).toBe(1));
  expect(panel.mounts).toBe(1);
  expect(
    screen.getByRole("region", { name: "Sample experience" }),
  ).toBeTruthy();
  expect(fake.calls.some((c) => c.name === "scan.start")).toBe(false);

  await act(async () =>
    gate.resolve({
      sessionRef: "sample-1",
      state: "closed",
      releasedVersions: 2,
    }),
  );
  await waitFor(() => expect(panel.mounts).toBe(2));
  expect(
    screen.queryByRole("region", { name: "Sample experience" }),
  ).toBeNull();
  expect((screen.getByLabelText("Draft path") as HTMLInputElement).value).toBe(
    "",
  );
  // Close carried only the session reference.
  expect(fake.calls.find((c) => c.name === "sample.close")?.body).toEqual({
    sessionRef: "sample-1",
  });
});

test("an INTERNAL warning on close still ends the sample", async () => {
  const fake = openedFixture(sampleSession(), {
    "sample.close": () => {
      throw new AnalysisError("INTERNAL", "sample.release_failed");
    },
  });
  await start(fake);
  fireEvent.click(button("End experience"));
  await confirmDialog("End experience");
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Sample experience" }),
    ).toBeNull(),
  );
});

test("a session the host already closed ends the sample screen with an explanation", async () => {
  const fake = openedFixture(sampleSession(), {
    "sample.status": () => {
      throw new AnalysisError("NOT_FOUND", "sample.session_closed");
    },
  });
  await start(fake);
  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "inactive",
        sequence: 2,
      }),
    );
  });
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Sample experience" }),
    ).toBeNull(),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "already ended",
  );
  expect(screen.getByLabelText("Draft path")).toBeTruthy();
});

test("buttons stay disabled while a command runs", async () => {
  const gate = deferred<ReturnType<typeof sampleSession>>();
  const fake = openedFixture(sampleSession(), {
    "sample.reset": () => gate.promise,
  });
  await start(fake);
  fireEvent.click(button("Reset experience"));
  const dialog = await screen.findByRole("alertdialog");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Reset experience" }),
  );
  await waitFor(() => expect(fake.count("sample.reset")).toBe(1));
  for (const name of ["Cancel", "Reset experience"]) {
    expect(
      (within(dialog).getByRole("button", { name }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  }
  await act(async () => gate.resolve(sampleSession({ sequence: 2 })));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect((button("End experience") as HTMLButtonElement).disabled).toBe(false);
  expect(fake.count("sample.reset")).toBe(1);
});
