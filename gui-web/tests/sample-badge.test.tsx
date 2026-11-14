import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import type { BrowseDetail, DataSource } from "../src/ipc";
import { sampleClient } from "./sample-support";

afterEach(cleanup);

function detail(dataSource?: DataSource): BrowseDetail {
  return {
    item: { resultVersion: "rv", itemNo: 1 },
    ...(dataSource ? { dataSource } : {}),
    displayPath: { state: "present", value: "Sample data\\videos" },
    displayName: { state: "present", value: "videos" },
    kind: { state: "present", value: "directory" },
    logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
    allocatedSize: { state: "unknown", reasonKey: "ref.unavailable" },
    createdAt: { state: "absent" },
    modifiedAt: { state: "absent" },
    observedAt: { state: "absent" },
    origin: "scan",
    completeness: "complete",
    currentCorrespondence: false,
    observationOnly: true,
  };
}

async function inspect(source?: DataSource) {
  const fake = sampleClient({ "browse.detail": () => detail(source) });
  render(
    <FileInspection
      client={fake.client}
      item={{ resultVersion: "rv", itemNo: 1 }}
      itemKind="folder"
    />,
  );
  await screen.findByText("Scan record");
}

test.each([
  [undefined, true],
  ["sample", true],
  ["real", false],
] as const)(
  "detail with data source %s shows the example badge: %s",
  async (source, shown) => {
    await inspect(source);
    expect(screen.queryByText("Example") !== null).toBe(shown);
  },
);
