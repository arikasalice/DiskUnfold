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
import type { EventEnvelope } from "../src/ipc";
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
  BrowsePanel: (props: {
    resultVersion?: string;
    rootLabel?: string;
    titleBadge?: unknown;
    actionsDisabledReason?: string;
  }) => (
    <div
      data-testid="sample-browse"
      data-version={props.resultVersion}
      data-root={props.rootLabel}
      data-reason={props.actionsDisabledReason}
    >
      {props.titleBadge as never}
    </div>
  ),
}));

beforeEach(() => {
  panel.mounts = 0;
});
afterEach(cleanup);

const button = (name: string) => screen.getByRole("button", { name });
const entry = () => button("Try sample data first");
const strip = () => screen.getByRole("region", { name: "Sample experience" });
/** What the sample live region speaks now. */
const announced = () =>
  document.querySelector(".du-sample__announcer")?.textContent ?? "";

async function confirmDialog(name: string) {
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("Only the sample state is discarded.");
  expect(dialog.textContent).toContain(
    "Your real analysis results, plans and settings stay as they are.",
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
  fireEvent.click(entry());
  await screen.findByRole("region", { name: "Sample experience" });
}

const progressEvent = (jobRef: string, completed: number): EventEnvelope => ({
  topic: "job.progress",
  seq: completed,
  contractVersion: 1,
  occurredAt: "2026-02-01T00:00:00Z",
  jobRef,
  body: {
    sequence: completed,
    unit: "items",
    completed,
    totalKnown: false,
    waitReason: "none",
    observedAt: "2026-02-01T00:00:00Z",
  },
});

test("the strip pins its words, then real analysis, then closing, with an icon and no bundle version", async () => {
  await start(openedFixture());
  expect(strip().textContent).toContain(
    "Sample experience · Separate from your files · Changes can't be run",
  );
  expect(strip().textContent).not.toContain("2026.1");
  expect(strip().querySelector("svg")).not.toBeNull();
  const stops = [
    ...strip().querySelectorAll<HTMLElement>("[tabindex='0'], button"),
  ];
  expect(stops.map((stop) => stop.textContent)).toEqual([
    "Sample experience · Separate from your files · Changes can't be run",
    "Start real analysis",
    "End experience",
  ]);
  // The strip is the first thing in the page body after the header.
  expect(strip().closest(".du-app")?.querySelector("section")).toBe(strip());
});

test("an active session browses its latest observation under the sample root and can switch time", async () => {
  await start(openedFixture());
  let browse = await screen.findByTestId("sample-browse");
  expect(browse.dataset.version).toBe("rv1");
  expect(browse.dataset.root).toBe("Sample data");
  // The Example mark sits beside the result title and file actions carry the reason.
  expect(within(browse).getByText("Example")).toBeTruthy();
  expect(browse.dataset.reason).toBe("Cannot run on sample data");
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
  const button1 = entry();
  fireEvent.click(button1);
  await waitFor(() =>
    expect((button1 as HTMLButtonElement).disabled).toBe(true),
  );
  // Progress shows at once, where the button was pressed; there is nothing to cancel yet.
  expect(
    screen.getByRole("progressbar", { name: "Preparing the sample data" }),
  ).toBeTruthy();
  expect((button("Cancel experience") as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(button1);
  expect(fake.count("sample.open")).toBe(1);
  await act(async () => gate.resolve(sampleSession()));
  await screen.findByRole("region", { name: "Sample experience" });
});

test("a busy start asks the user to try again at the entry", async () => {
  const fake = openedFixture(sampleSession(), {
    "sample.open": () => {
      throw new AnalysisError("BUSY", "job.queue_full", true);
    },
  });
  render(<App analysis={fake.client} />);
  fireEvent.click(entry());
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Try again in a moment",
  );
  expect(
    screen.queryByRole("region", { name: "Sample experience" }),
  ).toBeNull();
  expect(screen.getByLabelText("Draft path")).toBeTruthy();
});

test("preparing stays at the entry with a count and a cancel button, then the sample screen opens and says so", async () => {
  let current = sampleSession({ state: "preparing", observations: [] });
  const fake = openedFixture(current, {
    "sample.open": () => current,
    "sample.status": () => current,
  });
  render(<App analysis={fake.client} />);
  fireEvent.click(entry());
  await screen.findByRole("progressbar", { name: "Preparing the sample data" });
  // The real analysis screen stays and no strip shows yet.
  expect(screen.getByLabelText("Draft path")).toBeTruthy();
  expect(
    screen.queryByRole("region", { name: "Sample experience" }),
  ).toBeNull();
  expect(announced()).toBe("Preparing the sample data");
  expect(screen.queryByText(/example items? prepared/)).toBeNull();

  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "loading",
        sequence: 2,
        jobRef: "job-1",
      }),
    );
  });
  act(() => fake.emit(progressEvent("job-1", 1240)));
  expect(await screen.findByText("1240 example items prepared")).toBeTruthy();
  // Progress of another job is not this sample's.
  act(() => fake.emit(progressEvent("job-other", 5)));
  expect(screen.getByText("1240 example items prepared")).toBeTruthy();

  current = sampleSession({ sequence: 3 });
  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "active",
        sequence: 3,
      }),
    );
  });
  await screen.findByRole("region", { name: "Sample experience" });
  expect(announced()).toBe("Sample experience is ready");
  // The strip takes the focus the hidden entry button left behind.
  expect(document.activeElement?.textContent).toContain(
    "Separate from your files",
  );
});

test("cancelling the preparation releases the session without a dialog and returns the focus to the entry", async () => {
  const fake = openedFixture(
    sampleSession({ state: "preparing", observations: [] }),
  );
  render(<App analysis={fake.client} />);
  fireEvent.click(entry());
  await screen.findByRole("progressbar", { name: "Preparing the sample data" });
  fireEvent.click(button("Cancel experience"));
  await waitFor(() => expect(fake.count("sample.close")).toBe(1));
  expect(screen.queryByRole("alertdialog")).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(entry()));
  expect(announced()).toBe("");
});

test("a reset says it is resetting and then that it is done", async () => {
  let current = sampleSession();
  const fake = openedFixture(current, {
    "sample.reset": () =>
      (current = { ...current, state: "resetting", sequence: 2 }),
    "sample.status": () => current,
  });
  await start(fake);
  expect(announced()).toBe("Sample experience is ready");
  fireEvent.click(button("Reset experience"));
  await confirmDialog("Reset experience");
  await waitFor(() => expect(fake.count("sample.reset")).toBe(1));
  await waitFor(() =>
    expect(announced()).toBe("Resetting the sample experience"),
  );
  // Still on the sample screen with its strip while it resets.
  expect(strip()).toBeTruthy();
  expect(
    screen.getByRole("progressbar", {
      name: "Resetting the sample experience",
    }),
  ).toBeTruthy();
  current = { ...current, state: "active", sequence: 3 };
  act(() => {
    fake.emit(
      sampleEvent("sample.state_changed", {
        sessionRef: "sample-1",
        state: "active",
        sequence: 3,
      }),
    );
  });
  await waitFor(() => expect(announced()).toBe("Reset complete"));
});

test.each([
  ["sample.format_unsupported", "cannot read the example data format"],
  ["sample.bundle_invalid", "damaged"],
  ["sample.load_failed", "could not be read"],
  ["sample.something_new", "cannot be used right now"],
  [undefined, "cannot be used right now"],
])(
  "an unavailable sample (%s) is an error notice at the entry that keeps real analysis and retries without asking",
  async (reason, wording) => {
    const fake = openedFixture(
      sampleSession({
        state: "unavailable",
        observations: [],
        ...(reason ? { unavailableReason: reason } : {}),
      }),
    );
    render(<App analysis={fake.client} />);
    fireEvent.click(entry());
    const notice = await screen.findByRole("region", {
      name: "The sample experience is unavailable",
    });
    expect(notice.textContent).toContain("The sample cannot be opened · ");
    expect(notice.textContent).toContain(wording);
    expect(notice.textContent).toContain("You can still run a real analysis.");
    expect(notice.textContent).not.toContain("sample.");
    expect(notice.querySelector(".du-notice--error")).not.toBeNull();
    expect(
      within(notice)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Try again"]);
    // Real analysis stays on screen and the failure is spoken once.
    expect(screen.getByLabelText("Draft path")).toBeTruthy();
    expect(announced()).toContain("Sample experience unavailable.");

    fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(fake.count("sample.reset")).toBe(1));
  },
);

test("a sample missing from the installation is an information notice with no action", async () => {
  const fake = openedFixture(
    sampleSession({
      state: "unavailable",
      observations: [],
      unavailableReason: "sample.missing",
    }),
  );
  render(<App analysis={fake.client} />);
  fireEvent.click(entry());
  const notice = await screen.findByRole("region", {
    name: "The sample experience is unavailable",
  });
  expect(notice.textContent).toBe(
    "This installation does not include sample data. Choose a folder to start analyzing right away.",
  );
  expect(notice.querySelector(".du-notice--info")).not.toBeNull();
  expect(within(notice).queryAllByRole("button")).toEqual([]);
  expect(screen.getByLabelText("Draft path")).toBeTruthy();
});

test("a retry from the unavailable notice shows progress at the entry", async () => {
  const gate = deferred<ReturnType<typeof sampleSession>>();
  const fake = openedFixture(
    sampleSession({
      state: "unavailable",
      observations: [],
      unavailableReason: "sample.load_failed",
    }),
    { "sample.reset": () => gate.promise },
  );
  render(<App analysis={fake.client} />);
  fireEvent.click(entry());
  fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
  await screen.findByRole("progressbar", {
    name: "Resetting the sample experience",
  });
  expect(screen.getByLabelText("Draft path")).toBeTruthy();
  await act(async () => gate.resolve(sampleSession({ sequence: 2 })));
  await screen.findByRole("region", { name: "Sample experience" });
});

test("the end confirmation names what is lost, starts on Cancel and returns to End experience", async () => {
  const fake = openedFixture();
  await start(fake);
  const opener = button("End experience");
  act(() => opener.focus());
  fireEvent.click(opener);
  const dialog = await screen.findByRole("alertdialog");
  expect(
    within(dialog).getByRole("heading", {
      name: "End the sample experience?",
    }),
  ).toBeTruthy();
  expect(
    within(dialog)
      .getAllByRole("button")
      .map((b) => b.textContent),
  ).toEqual(["Cancel", "End experience"]);
  expect(document.activeElement).toBe(
    within(dialog).getByRole("button", { name: "Cancel" }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(document.activeElement).toBe(opener));
  expect(fake.count("sample.close")).toBe(0);
});

test("ending the experience returns the focus to the entry button", async () => {
  const fake = openedFixture();
  await start(fake);
  fireEvent.click(button("End experience"));
  await confirmDialog("End experience");
  await waitFor(() => expect(fake.count("sample.close")).toBe(1));
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Sample experience" }),
    ).toBeNull(),
  );
  await waitFor(() => expect(document.activeElement).toBe(entry()));
});

test("switching to real analysis remounts the target screen only after close answered and focuses the entry", async () => {
  const gate = deferred<unknown>();
  const fake = openedFixture(sampleSession(), {
    "sample.close": () => gate.promise,
  });
  render(<App analysis={fake.client} />);
  expect(panel.mounts).toBe(1);
  fireEvent.change(screen.getByLabelText("Draft path"), {
    target: { value: "D:\\old selection" },
  });
  fireEvent.click(entry());
  await screen.findByRole("region", { name: "Sample experience" });

  fireEvent.click(button("Start real analysis"));
  await confirmDialog("Start real analysis");
  await waitFor(() => expect(fake.count("sample.close")).toBe(1));
  expect(panel.mounts).toBe(1);
  expect(strip()).toBeTruthy();
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
  await waitFor(() => expect(document.activeElement).toBe(entry()));
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

test("a session the host already closed ends the sample screen with an explanation at the entry", async () => {
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
