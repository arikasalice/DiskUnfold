import { useEffect, useState } from "react";
import {
  DetailDisclosure,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  BrowseValue,
  ItemRef,
} from "../../ipc";
import { CopyPath } from "./CopyPath";
import { ItemSpaceDetails } from "./ItemSpaceDetails";
import {
  displaySizeWithBytes,
  displaySpaceReason,
  evidenceLabels,
  windowsSizeDisplay,
} from "./space-format";
import "./file-inspection.css";
import type { SizeDisplay } from "./space-format";

function attribute<T>(
  field: BrowseValue<T>,
  format: (value: T) => string = String,
) {
  if (field.state === "present") return format(field.value);
  if (field.state === "absent") return "Not recorded";
  return (
    displaySpaceReason(field.reasonKey) ??
    `Unavailable in this scan (${field.reasonKey})`
  );
}

function evidence(value: BrowseDetail["logicalSize"]): string[] {
  if (value.state !== "present") return [];
  const label = evidenceLabels[value.value.evidence];
  return label ? [label] : [];
}

function attributes(
  detail: BrowseDetail,
  itemKind: "file" | "folder",
  display: SizeDisplay,
  client: AnalysisClient,
  item: ItemRef,
) {
  const size = (value: BrowseDetail["logicalSize"]) =>
    attribute(value, (observed) => displaySizeWithBytes(observed, display));
  return [
    {
      label: itemKind === "file" ? "File name" : "Folder name",
      wideValue: true,
      value:
        detail.displayName.state === "present" ? (
          <div className="du-inspection-name">
            <Text>{detail.displayName.value}</Text>
            <DetailDisclosure title="Full name">
              <Text focusable>{detail.displayName.value}</Text>
            </DetailDisclosure>
          </div>
        ) : (
          attribute(detail.displayName)
        ),
    },
    {
      label: "Logical size",
      value: size(detail.logicalSize),
      notes: evidence(detail.logicalSize),
    },
    {
      label: "Allocated size",
      value: size(detail.allocatedSize),
      notes: evidence(detail.allocatedSize),
    },
    { label: "Result published", value: attribute(detail.observedAt) },
    { label: "Source", value: detail.observationOnly ? "Scan record" : "Scan" },
    {
      label: "Path",
      wideValue: true,
      value: <CopyPath client={client} item={item} path={detail.displayPath} />,
    },
    { label: "Kind", value: attribute(detail.kind) },
  ];
}

/** Reads attributes and space for the file in the browser's pinned result. */
export function FileInspection({
  client,
  item,
  itemKind = "file",
  display = windowsSizeDisplay,
}: {
  client: AnalysisClient;
  item: ItemRef;
  itemKind?: "file" | "folder";
  display?: SizeDisplay;
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
          setProblem(`The attributes do not match this ${itemKind}.`);
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        setProblem(
          failure instanceof AnalysisError && failure.code === "STALE_VERSION"
            ? `Open the latest result to see its ${itemKind} details.`
            : `${itemKind === "file" ? "File" : "Folder"} attributes are unavailable.`,
        );
      });
    return () => {
      active = false;
    };
  }, [client, resultVersion, itemNo, itemKind]);

  return (
    <>
      <Text kind="muted">Information observed during the scan.</Text>
      {problem && <Notice tone="error">{problem}</Notice>}
      {!problem && !detail && (
        <Progress label={`Reading ${itemKind} attributes`} />
      )}
      {detail &&
        detail.item.resultVersion === resultVersion &&
        detail.item.itemNo === itemNo && (
          <>
            <Readout
              label={`Observed ${itemKind} attributes`}
              rows={attributes(detail, itemKind, display, client, item)}
            />
            <DetailDisclosure title="More observed attributes">
              <Readout
                label="More observed attributes"
                rows={[
                  { label: "Modified", value: attribute(detail.modifiedAt) },
                  { label: "Created", value: attribute(detail.createdAt) },
                ]}
              />
            </DetailDisclosure>
          </>
        )}
      <ItemSpaceDetails client={client} item={item} display={display} />
    </>
  );
}
