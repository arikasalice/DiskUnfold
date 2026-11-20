import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { BrowseScanStatusPreview } from "../preview/BrowseScanStatusPreview";

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

test.each([
  ["scan-status", 6],
  ["lower-bounds", 3],
  ["lower-bounds-notice", 3],
])("browse gallery scene %s renders %i rows", async (scene, count) => {
  window.history.replaceState(null, "", `/?surface=browse&scene=${scene}`);
  render(<BrowseScanStatusPreview />);
  await waitFor(() =>
    expect(document.querySelectorAll(".du-browse-grid__row")).toHaveLength(
      count,
    ),
  );
});
