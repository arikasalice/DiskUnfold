import { describe, expect, it } from "vitest";
import { folderPath } from "../src/features/browse/browse-display";
import type { BrowseOpenResponse } from "../src/ipc";

type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;

const version = "0195f856-ab83-7a59-9343-10d29775c9a9";
const crumb = (itemNo: number, name: string) => ({
  item: { resultVersion: version, itemNo },
  displayName: { state: "present" as const, value: name },
});

function opened(): Opened {
  return {
    kind: "opened",
    rootKind: "folder",
    location: { resultVersion: version, itemNo: 2 },
    breadcrumbs: [crumb(0, "/sample"), crumb(2, "Projects")],
    page: { resultVersion: version },
  } as unknown as Opened;
}

describe("folderPath root label", () => {
  it("names a single scope root with the supplied label", () => {
    const labels = folderPath(
      opened(),
      false,
      () => {},
      undefined,
      "Sample data",
    ).map((entry) => entry.label);
    expect(labels).toEqual(["Sample data", "Projects"]);
  });

  it("keeps the native root name without a label", () => {
    const labels = folderPath(opened(), false, () => {}).map(
      (entry) => entry.label,
    );
    expect(labels).toEqual(["/sample", "Projects"]);
  });
});
