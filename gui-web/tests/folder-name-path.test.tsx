import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import type { BrowseDetail } from "../src/ipc";
import { sampleClient } from "./sample-support";

/*
 * #171, F02 README: a scope root names itself with its whole path. Its name
 * row then follows the path rule and breaks after a separator first; a plain
 * folder name stays plain text.
 */

afterEach(cleanup);

const root =
  "C:\\Users\\runneradmin\\AppData\\Local\\Temp\\diskunfold-smoke-24b69567\\scope";

function detail(name: string): BrowseDetail {
  return {
    item: { resultVersion: "rv", itemNo: 1 },
    displayPath: { state: "present", value: root },
    displayName: { state: "present", value: name },
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

async function nameRow(name: string) {
  const fake = sampleClient({ "browse.detail": () => detail(name) });
  render(
    <FileInspection
      client={fake.client}
      item={{ resultVersion: "rv", itemNo: 1 }}
      itemKind="folder"
    />,
  );
  await screen.findByText("Scan record");
  return screen.getByText("Folder name").closest("div")!.querySelector("dd")!;
}

test("#171: a folder name that is a whole path breaks after its separators", async () => {
  const row = await nameRow(root);
  expect(row.querySelectorAll(".du-path")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /Full name/ }));
  const paths = row.querySelectorAll(".du-path");
  // The shown name and the one under "Full name" both follow the rule.
  expect(paths).toHaveLength(2);
  for (const path of paths) {
    expect(path.textContent).toBe(root);
    expect(path.querySelectorAll("wbr")).toHaveLength(7);
    expect(path.querySelector(".du-path__part")?.textContent).toBe(
      "diskunfold-smoke-24b69567\\",
    );
  }
});

test("#171: a plain folder name stays plain text", async () => {
  const row = await nameRow("scope");
  expect(row.querySelector(".du-path")).toBeNull();
  expect(row.querySelector("wbr")).toBeNull();
});
