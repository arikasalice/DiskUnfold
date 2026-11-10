import { useEffect, useState } from "react";
import { Notice, Progress, Readout, Text } from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  BrowseValue,
  ItemRef,
} from "../../ipc";
import { ItemSpaceDetails } from "./ItemSpaceDetails";
import { displaySizeWithBytes, windowsSizeDisplay } from "./space-format";

function attribute<T>(
  field: BrowseValue<T>,
  format: (value: T) => string = String,
) {
  if (field.state === "present") return format(field.value);
  if (field.state === "absent") return "Not recorded";
  return "Unavailable in this scan";
}

function attributes(detail: BrowseDetail) {
  const size = (value: BrowseDetail["logicalSize"]) =>
    attribute(value, (observed) =>
      displaySizeWithBytes(observed, windowsSizeDisplay),
    );
  return [
    { label: "File name", value: attribute(detail.displayName) },
    {
      label: "Path",
      value:
        detail.displayPath.state === "present" ? (
          <Text kind="path" focusable>
            {detail.displayPath.value}
          </Text>
        ) : (
          attribute(detail.displayPath)
        ),
    },
    { label: "Kind", value: attribute(detail.kind) },
    { label: "Logical size", value: size(detail.logicalSize) },
    { label: "Allocated size", value: size(detail.allocatedSize) },
    { label: "Modified", value: attribute(detail.modifiedAt) },
    { label: "Created", value: attribute(detail.createdAt) },
    { label: "Result published", value: attribute(detail.observedAt) },
    { label: "Source", value: detail.observationOnly ? "Scan record" : "Scan" },
  ];
}

/** Reads attributes and space for the file in the browser's pinned result. */
export function FileInspection({
  client,
  item,
}: {
  client: AnalysisClient;
  item: ItemRef;
}) {
  const [detail, setDetail] = useState<BrowseDetail>();
  const [problem, setProblem] = useState<string>();
  const { resultVersion, itemNo } = item;
  useEffect(() => {
    let active = true;
    void client
      .call("browse.detail", {
        resultVersion,
        item: { resultVersion, itemNo },
      })
      .then((response) => {
        if (!active) return;
        if (
          response.item.resultVersion !== resultVersion ||
          response.item.itemNo !== itemNo
        ) {
          setProblem("The attributes do not match this file.");
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        setProblem(
          failure instanceof AnalysisError && failure.code === "STALE_VERSION"
            ? "Open the latest result to see its file details."
            : "File attributes are unavailable.",
        );
      });
    return () => {
      active = false;
    };
  }, [client, resultVersion, itemNo]);

  return (
    <>
      <Text kind="muted">Information observed during the scan.</Text>
      {problem && <Notice tone="error">{problem}</Notice>}
      {!problem && !detail && <Progress label="Reading file attributes" />}
      {detail && (
        <Readout label="Observed file attributes" rows={attributes(detail)} />
      )}
      <ItemSpaceDetails
        client={client}
        item={item}
        display={windowsSizeDisplay}
      />
    </>
  );
}
