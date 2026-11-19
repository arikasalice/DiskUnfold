import { renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useVisualizationOffer } from "../src/app/browse-visualization-offer";
import type { ReportedBasis } from "../src/app/browse-visualization-offer";
import type { BrowseOpenLocation } from "../src/features/browse/BrowsePanel";

/*
 * #155: right after a scan ends, "Browse folders" opens the scan's newest
 * result while the analysis summary still reports the size basis of the
 * earlier one. The space view is offered for the opened folder once the
 * summary reports the basis of that same result, and never with the basis of
 * another result.
 */

const display = { system: "iec", unit: "auto" } as const;

function folder(version: string): BrowseOpenLocation {
  const location = { resultVersion: version, itemNo: 1 };
  return {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location,
    breadcrumbs: [
      {
        item: location,
        displayName: { state: "present", value: "Photos" },
      },
    ],
    rootKind: "folder",
    currentCorrespondence: true,
    sortStrategies: [],
    predicateStrategies: [],
    page: { resultVersion: version },
  } as unknown as BrowseOpenLocation;
}

function offer(initial: ReportedBasis) {
  const opened = vi.fn();
  const hook = renderHook(
    ({ reported }) => useVisualizationOffer(reported, display, opened),
    { initialProps: { reported: initial } },
  );
  return { opened, hook };
}

test("#155: the space view waits for the basis of the opened result", () => {
  const { opened, hook } = offer({ version: "v1", basis: "logical" });
  hook.result.current.browsed("v2", folder("v2"));
  expect(opened).toHaveBeenLastCalledWith(undefined);

  hook.rerender({ reported: { version: "v2", basis: "allocated" } });
  expect(opened).toHaveBeenLastCalledWith(
    expect.objectContaining({ resultVersion: "v2", sizeBasis: "allocated" }),
  );
  expect(opened).toHaveBeenCalledTimes(2);

  // Offered once: a later report of the same result does not hand it again.
  hook.rerender({ reported: { version: "v2", basis: "logical" } });
  expect(opened).toHaveBeenCalledTimes(2);
});

test("#155: a folder whose basis is known is offered at once", () => {
  const { opened, hook } = offer({ version: "v2", basis: "logical" });
  hook.result.current.browsed("v2", folder("v2"));
  expect(opened).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ resultVersion: "v2", sizeBasis: "logical" }),
  );
});

test("#155: another opening forgets the folder", () => {
  const { opened, hook } = offer({ version: "v1", basis: "logical" });
  hook.result.current.browsed("v2", folder("v2"));
  hook.result.current.opening();
  hook.rerender({ reported: { version: "v2", basis: "logical" } });
  expect(opened).toHaveBeenCalledExactlyOnceWith(undefined);
});
