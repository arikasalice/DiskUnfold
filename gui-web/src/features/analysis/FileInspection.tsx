import { useEffect, useState } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import {
  DetailDisclosure,
  ExampleBadge,
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
import { isSampleSource } from "../../state/sample-session";
import { ItemSpaceDetails } from "./ItemSpaceDetails";
import {
  displaySizeParts,
  displaySpaceReason,
  evidenceLabels,
  windowsSizeDisplay,
} from "./space-format";
import "./file-inspection.css";
import type { SizeDisplay } from "./space-format";

function attribute<T>(
  field: BrowseValue<T>,
  format: (value: T) => string = String,
  text?: LocaleText,
) {
  if (field.state === "present") return format(field.value);
  if (field.state === "absent")
    return text?.("ui.analysis.not_recorded", "Not recorded") ?? "Not recorded";
  return (
    displaySpaceReason(field.reasonKey) ??
    text?.("ui.inspection.unavailable_in_scan", "Unavailable in this scan") ??
    "Unavailable in this scan"
  );
}

function diagnostics(detail: BrowseDetail, text: LocaleText) {
  const fields = [
    [text("ui.inspection.name", "Name"), detail.displayName],
    [text("ui.inspection.path", "Path"), detail.displayPath],
    [text("ui.inspection.kind", "Kind"), detail.kind],
    [text("ui.analysis.logical_size", "Logical size"), detail.logicalSize],
    [
      text("ui.analysis.allocated_size", "Allocated size"),
      detail.allocatedSize,
    ],
    [text("ui.inspection.modified", "Modified"), detail.modifiedAt],
    [text("ui.inspection.created", "Created"), detail.createdAt],
    [text("ui.inspection.published", "Result published"), detail.observedAt],
  ] as const;
  return fields.flatMap(([label, field]) =>
    field.state === "unknown" ? [{ label, value: field.reasonKey }] : [],
  );
}

function sizeRow(
  label: string,
  field: BrowseDetail["logicalSize"],
  display: SizeDisplay,
  text: LocaleText,
) {
  if (field.state !== "present") {
    return {
      label,
      emphasized: true,
      value: attribute(field, String, text),
      notes: [],
    };
  }
  const { formatted, raw } = displaySizeParts(field.value, display, text);
  const certainty =
    field.value.state === "exact"
      ? text("ui.inspection.exact", "Exact")
      : undefined;
  const evidence = evidenceLabels[field.value.evidence];
  return {
    label,
    emphasized: true,
    value: formatted,
    notes: [raw, certainty, evidence].filter(
      (note): note is string => note !== undefined,
    ),
  };
}

function attributes(
  detail: BrowseDetail,
  itemKind: "file" | "folder",
  display: SizeDisplay,
  client: AnalysisClient,
  item: ItemRef,
  text: LocaleText,
  renderTime: (value: string) => string,
) {
  return [
    {
      label:
        itemKind === "file"
          ? text("ui.inspection.file_name", "File name")
          : text("ui.inspection.folder_name", "Folder name"),
      wideValue: true,
      value:
        detail.displayName.state === "present" ? (
          <div className="du-inspection-name">
            <Text>{detail.displayName.value}</Text>
            <DetailDisclosure
              title={text("ui.inspection.full_name", "Full name")}
            >
              <Text focusable>{detail.displayName.value}</Text>
            </DetailDisclosure>
          </div>
        ) : (
          attribute(detail.displayName, String, text)
        ),
    },
    sizeRow(
      text("ui.analysis.logical_size", "Logical size"),
      detail.logicalSize,
      display,
      text,
    ),
    sizeRow(
      text("ui.analysis.allocated_size", "Allocated size"),
      detail.allocatedSize,
      display,
      text,
    ),
    {
      label: text("ui.inspection.published", "Result published"),
      value: attribute(detail.observedAt, renderTime, text),
    },
    {
      label: text("ui.inspection.source", "Source"),
      value: (
        <>
          {detail.observationOnly
            ? text("ui.inspection.scan_record", "Scan record")
            : text("ui.inspection.scan", "Scan")}
          {isSampleSource(detail.dataSource) && (
            <>
              {" "}
              <ExampleBadge>
                {text("ui.sample.example_badge", "Example")}
              </ExampleBadge>
            </>
          )}
        </>
      ),
    },
    {
      label: text("ui.inspection.path", "Path"),
      wideValue: true,
      value: <CopyPath client={client} item={item} path={detail.displayPath} />,
    },
    {
      label: text("ui.inspection.kind", "Kind"),
      value: attribute(detail.kind, String, text),
    },
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
  const [problem, setProblem] = useState<
    "mismatch" | "stale" | "unavailable"
  >();
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const renderTime = (value: string) =>
    timestamp(Date.parse(value), () => value);
  const kind =
    itemKind === "file"
      ? text("ui.inspection.file", "file")
      : text("ui.inspection.folder", "folder");
  const reasons = text("ui.inspection.reasons", "Diagnostic reasons");
  const more = text("ui.inspection.more", "More observed attributes");
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
          setProblem("mismatch");
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        setProblem(
          failure instanceof AnalysisError && failure.code === "STALE_VERSION"
            ? "stale"
            : "unavailable",
        );
      });
    return () => {
      active = false;
    };
  }, [client, resultVersion, itemNo, itemKind]);

  return (
    <>
      <Text kind="muted">
        {text(
          "ui.inspection.observed_hint",
          "Information observed during the scan.",
        )}
      </Text>
      {problem && (
        <Notice tone="error">
          {inspectionProblem(problem, itemKind, kind, text)}
        </Notice>
      )}
      {!problem && !detail && (
        <Progress
          label={text("ui.inspection.loading", "Reading {kind} attributes", {
            kind,
          })}
        />
      )}
      {detail &&
        detail.item.resultVersion === resultVersion &&
        detail.item.itemNo === itemNo && (
          <>
            <div className="du-inspection-summary">
              <Readout
                label={text(
                  "ui.inspection.observed",
                  "Observed {kind} attributes",
                  { kind },
                )}
                rows={attributes(
                  detail,
                  itemKind,
                  display,
                  client,
                  item,
                  text,
                  renderTime,
                )}
              />
            </div>
            {diagnostics(detail, text).length > 0 && (
              <DetailDisclosure title={reasons}>
                <Readout label={reasons} rows={diagnostics(detail, text)} />
              </DetailDisclosure>
            )}
            <DetailDisclosure title={more}>
              <Readout
                label={more}
                rows={[
                  {
                    label: text("ui.inspection.modified", "Modified"),
                    value: attribute(detail.modifiedAt, renderTime, text),
                  },
                  {
                    label: text("ui.inspection.created", "Created"),
                    value: attribute(detail.createdAt, renderTime, text),
                  },
                ]}
              />
            </DetailDisclosure>
          </>
        )}
      <ItemSpaceDetails client={client} item={item} display={display} />
    </>
  );
}

function inspectionProblem(
  problem: "mismatch" | "stale" | "unavailable",
  itemKind: "file" | "folder",
  kind: string,
  text: LocaleText,
) {
  if (problem === "mismatch")
    return text(
      "ui.inspection.mismatch",
      "The attributes do not match this {kind}.",
      { kind },
    );
  if (problem === "stale")
    return text(
      "ui.inspection.stale",
      "Open the latest result to see its {kind} details.",
      { kind },
    );
  return itemKind === "file"
    ? text("ui.inspection.file_unavailable", "File attributes are unavailable.")
    : text(
        "ui.inspection.folder_unavailable",
        "Folder attributes are unavailable.",
      );
}
