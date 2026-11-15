import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { useTimestampParts } from "../../i18n/regional-format";
import {
  DetailDisclosure,
  ExampleBadge,
  Moment,
  NameText,
  Notice,
  Progress,
  Readout,
  Text,
  Unbroken,
} from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  BrowseDetail,
  BrowseValue,
  ItemRef,
} from "../../ipc";
import { CopyPath, OwedCopyStatus, owedCopyFailure } from "./CopyPath";
import type { CopyContinuation } from "./CopyPath";
import { isSampleSource } from "../../state/sample-session";
import { ItemSpaceDetails } from "./ItemSpaceDetails";
import {
  displayEvidence,
  displaySizeParts,
  displaySpaceReason,
  windowsSizeDisplay,
} from "./space-format";
import "./file-inspection.css";
import type { SizeDisplay } from "./space-format";

function attribute<T>(
  field: BrowseValue<T>,
  format: (value: T) => ReactNode,
  text: LocaleText,
): ReactNode {
  if (field.state === "present") return format(field.value);
  if (field.state === "absent")
    return text("ui.analysis.not_recorded", "Not recorded");
  return (
    displaySpaceReason(field.reasonKey, text) ??
    text("ui.inspection.unavailable_in_scan", "Unavailable in this scan")
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
  const evidence = displayEvidence(field.value.evidence, text);
  return {
    label,
    emphasized: true,
    value: <Unbroken>{formatted}</Unbroken>,
    notes: [raw, certainty, evidence].filter(
      (note): note is string => note !== undefined,
    ),
  };
}

/** What the path's copy action takes beyond the item and its path. */
type PathActions = {
  disabledReason?: string;
  onStale?: (resultVersion: string) => void;
  continuation?: CopyContinuation;
};

function attributes(
  detail: BrowseDetail,
  itemKind: "file" | "folder",
  display: SizeDisplay,
  client: AnalysisClient,
  item: ItemRef,
  text: LocaleText,
  renderTime: (value: string) => ReactNode,
  actions: PathActions,
) {
  return [
    {
      label:
        itemKind === "file"
          ? text("ui.inspection.file_name", "File name")
          : text("ui.inspection.folder_name", "Folder name"),
      value:
        detail.displayName.state === "present" ? (
          <div className="du-inspection-name">
            <Text>
              <NameText name={detail.displayName.value} />
            </Text>
            <DetailDisclosure
              title={text("ui.inspection.full_name", "Full name")}
            >
              <Text focusable>
                <NameText name={detail.displayName.value} />
              </Text>
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
      value: (
        <CopyPath
          client={client}
          item={item}
          path={detail.displayPath}
          {...actions}
        />
      ),
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
  actionsDisabledReason,
  onStale,
  copy,
}: {
  client: AnalysisClient;
  item: ItemRef;
  itemKind?: "file" | "folder";
  display?: SizeDisplay;
  /** Turns off the actions that act on a real path and shows this reason (the sample experience). */
  actionsDisabledReason?: string;
  /**
   * Told when a read or copy for `item` was refused because its result was
   * replaced, so a dialog can move to the newest result.
   */
  onStale?: ((resultVersion: string) => void) | undefined;
  /**
   * A dialog's copy that continues once on this item after a refusal. If
   * the item's detail cannot be read, the copy is reported as not done.
   */
  copy?: CopyContinuation | undefined;
}) {
  const [detail, setDetail] = useState<BrowseDetail>();
  const [problem, setProblem] = useState<
    "mismatch" | "stale" | "unavailable"
  >();
  const [owedFailure, setOwedFailure] = useState<"missing" | "failed">();
  const text = useLocaleText();
  const timestamp = useTimestampParts();
  const renderTime = (value: string) => (
    <Moment {...timestamp(Date.parse(value), () => value)} />
  );
  const kind =
    itemKind === "file"
      ? text("ui.inspection.file", "file")
      : text("ui.inspection.folder", "folder");
  const reasons = text("ui.inspection.reasons", "Diagnostic reasons");
  const more = text("ui.inspection.more", "More observed attributes");
  const { resultVersion, itemNo } = item;
  const actions: PathActions = {
    ...(actionsDisabledReason === undefined
      ? {}
      : { disabledReason: actionsDisabledReason }),
    ...(onStale === undefined ? {} : { onStale }),
    ...(copy === undefined ? {} : { continuation: copy }),
  };
  /** An owed copy cannot run without the path; say so once. */
  function failOwedCopy(failure: "missing" | "failed") {
    if (!copy?.pending) return;
    copy.done();
    setOwedFailure(failure);
  }
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
          failOwedCopy("failed");
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        const stale =
          failure instanceof AnalysisError && failure.code === "STALE_VERSION";
        setProblem(stale ? "stale" : "unavailable");
        if (stale) onStale?.(resultVersion);
        else failOwedCopy(owedCopyFailure(failure));
      });
    return () => {
      active = false;
    };
    // onStale and copy only report; new callbacks do not call for a read.
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
      {copy && <OwedCopyStatus failure={owedFailure} />}
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
                  actions,
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
      <ItemSpaceDetails
        client={client}
        item={item}
        display={display}
        onStale={onStale}
      />
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
