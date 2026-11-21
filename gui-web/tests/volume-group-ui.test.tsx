import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  ResultGroupNotice,
  ScopeReviewFeedback,
} from "../src/features/analysis/AnalysisActions";
import { TargetPicker } from "../src/features/analysis/TargetPicker";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { ScopeReview } from "../src/ipc";
import { createAnalysisClient, unixPath, windowsPath } from "../src/ipc";
import type { OsPath } from "../src/ipc";
import type { useTargets } from "../src/state/analysis-targets";
import { folderDetail } from "./summary-fixture";

afterEach(cleanup);

const path = windowsPath("C:\\fixture");
const localeText: LocaleText = (_key, fallback, args) =>
  Object.entries(args ?? {}).reduce(
    (message, [name, value]) => message.replaceAll(`{${name}}`, String(value)),
    fallback ?? "",
  );
const targets = {
  list: {
    targets: [],
    probeJobRef: null,
    observedAt: "2026-11-20T00:00:00.000Z",
    unavailableReason: null,
  },
  probes: {},
  loading: false,
  error: undefined,
  finished: true,
  refresh: async () => {},
  cancel: () => {},
} as ReturnType<typeof useTargets>;

function ScopeFeedback({
  notices,
  suggestedInclude,
  initialPaths = [path],
}: {
  notices: string[];
  suggestedInclude?: OsPath;
  initialPaths?: OsPath[];
}) {
  const [selected, setSelected] = useState(initialPaths);
  const review: ScopeReview = {
    scope: {
      includes: selected,
      exclusions: [],
      boundary: "current_volume",
      includedMounts: [],
      source: "gui",
    },
    notices,
    targets: [{ path, access: "accessible", reasonKey: null }],
    reviewFingerprint: "fingerprint",
    observedAt: "2026-11-20T00:00:00.000Z",
    requestSeq: 1,
    ...(suggestedInclude ? { suggestedInclude } : {}),
  };
  return (
    <LocaleTextContext.Provider value={localeText}>
      <TargetPicker
        targets={targets}
        disabled={false}
        selected={selected}
        choose={(next, index = 0) => {
          if (!next) return;
          setSelected((current) =>
            current.map((currentPath, pathIndex) =>
              pathIndex === index ? next : currentPath,
            ),
          );
        }}
        addPath={() => {}}
        removePath={(index) =>
          setSelected((current) =>
            current.filter((_, pathIndex) => pathIndex !== index),
          )
        }
        dropEnabled={false}
        dropError={undefined}
        pathDescription=""
        reviewDescriptionId={undefined}
        feedback={(onChooseFolder) => (
          <ScopeReviewFeedback
            review={review}
            ready
            current
            latestAccepted
            problem={undefined}
            problemId="scope-warning"
            onChooseFolder={onChooseFolder}
          />
        )}
        isInitial={false}
        onPathCompositionChange={() => {}}
        review={{
          reviewing: false,
          awaiting: false,
          targets: review.targets,
          openPrivacySettings: () => {},
        }}
      />
      <output data-testid="selected-paths">
        {selected.map((entry) => entry.display).join(",")}
      </output>
    </LocaleTextContext.Provider>
  );
}

test("F03 #263 merged notice is informational and stays in scope review", () => {
  render(<ScopeFeedback notices={["scope.volume_group_merged"]} />);
  const message = screen.getByText(
    "The system and data volumes are analyzed together. Each file is counted once.",
  );
  const notice = message.closest(".du-notice--info");
  expect(notice).toBeTruthy();
  expect(notice?.textContent).not.toContain("scope.volume_group_merged");
  const boundary = screen.getByLabelText("Scope paths and boundary");
  expect(
    boundary.compareDocumentPosition(notice!) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

test("F03 #263 unverified notice warns and its action focuses the path input", () => {
  const { container } = render(
    <ScopeFeedback notices={["scope.volume_group_unverified"]} />,
  );
  const notice = container.querySelector(".du-notice--warning");
  expect(notice?.textContent).toContain(
    "The data volume couldn't be confirmed, so only the system volume is analyzed. User files aren't included.",
  );
  const input = screen.getByRole("textbox", { name: "Folder path" });
  fireEvent.click(screen.getByRole("button", { name: "Choose a folder" }));
  expect(document.activeElement).toBe(input);
});

test("F03 #263 suggested home action replaces the draft with that one path", () => {
  const home = unixPath("/Users/fixture");
  const extra = windowsPath("C:\\extra");
  render(
    <ScopeFeedback
      notices={["scope.volume_group_unverified"]}
      suggestedInclude={home}
      initialPaths={[path, extra]}
    />,
  );
  const action = screen.getByRole("button", {
    name: /Analyze home folder/,
  });
  expect(action.firstChild?.textContent).toBe("Analyze home folder (");
  expect(action.lastChild?.textContent).toBe(")");
  expect(action.querySelector(".du-fit--line")).toBeTruthy();
  fireEvent.click(action);
  expect(screen.getByRole("textbox", { name: "Folder path" })).toHaveProperty(
    "value",
    home.display,
  );
  expect(screen.getByTestId("selected-paths").textContent).toBe(home.display);
});

test("F03 #263 countedAtItem opens the same-version folder through the browser", async () => {
  const navigateItem = vi.fn();
  const client = createAnalysisClient(async (_channel, { envelope }) => ({
    ...envelope,
    body: envelope.name === "browse.detail" ? folderDetail("v1") : {},
  }));
  render(
    <LocaleTextContext.Provider value={localeText}>
      <FileInspection
        client={client}
        item={{ resultVersion: "v1", itemNo: 0 }}
        itemKind="folder"
        countedAt={unixPath("/Users")}
        countedAtItem={{ resultVersion: "v1", itemNo: 4 }}
        navigateItem={navigateItem}
      />
    </LocaleTextContext.Provider>,
  );
  await screen.findByText("fixture");
  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  expect(navigateItem).toHaveBeenCalledWith({ resultVersion: "v1", itemNo: 4 });
});

test("F03 #263 result warning is shown only for an unverified group", () => {
  const { container, rerender } = render(
    <LocaleTextContext.Provider value={localeText}>
      <ResultGroupNotice unverified={false} />
    </LocaleTextContext.Provider>,
  );
  expect(container.querySelector(".du-notice--info")).toBeNull();
  expect(container.querySelector(".du-notice--warning")).toBeNull();
  rerender(
    <LocaleTextContext.Provider value={localeText}>
      <ResultGroupNotice unverified />
    </LocaleTextContext.Provider>,
  );
  expect(container.querySelector(".du-notice--warning")?.textContent).toContain(
    "User files aren't included.",
  );
});

test.each([true, false])(
  "F03 #263 countedAt detail row is %s only when supplied",
  async (withCountedAt) => {
    const client = createAnalysisClient(async (_channel, { envelope }) => ({
      ...envelope,
      body: envelope.name === "browse.detail" ? folderDetail("v1") : {},
    }));
    const props = {
      client,
      item: { resultVersion: "v1", itemNo: 0 },
      itemKind: "folder" as const,
    };
    const view = render(
      <LocaleTextContext.Provider value={localeText}>
        <FileInspection
          {...props}
          {...(withCountedAt ? { countedAt: unixPath("/Users") } : {})}
        />
      </LocaleTextContext.Provider>,
    );
    if (withCountedAt) {
      expect(await screen.findByText("Counted at")).toBeTruthy();
      expect(screen.getByText("/Users")).toBeTruthy();
    } else {
      await screen.findByText("fixture");
      expect(screen.queryByText("Counted at")).toBeNull();
    }
    view.unmount();
  },
);
