import type { ReactNode } from "react";
import {
  DetailDisclosure,
  InspectionDialog,
  NameText,
  Notice,
  Readout,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import {
  displayMinute,
  useRegionalFormat,
  useTimestampText,
} from "../../i18n/regional-format";
import type { BrowseOpenResponse, ItemRef } from "../../ipc";
import { folderPath, rowKey } from "./browse-display";
import type { InspectedItem } from "./use-item-inspection";
import type { LocaleText } from "../../i18n/presentation";

export type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;

/**
 * Names the origin of a result in the reader's language. The origin is an
 * open identifier; a value without copy reads as unknown and is not shown.
 */
function originText(origin: string, text: LocaleText): string {
  switch (origin) {
    case "scan":
      return text("ui.browse.origin_scan", "Current analysis");
    case "incremental":
      return text("ui.browse.origin_incremental", "Incremental update");
    default:
      return text("ui.common.unknown", "Unknown");
  }
}
export type ParentLocation = { item?: ItemRef };
export type InspectItem = (
  item: ItemRef,
  kind: InspectedItem["kind"],
) => ReactNode;
export function BrowseItemDialog({
  inspection,
  inspectItem,
  close,
}: {
  inspection: InspectedItem | undefined;
  inspectItem: InspectItem | undefined;
  close: () => void;
}) {
  const text = useLocaleText();
  if (!inspection || !inspectItem) return null;
  const title =
    inspection.kind === "folder"
      ? text("ui.browse.folder_details", "Folder details")
      : text("ui.browse.file_details", "File details");
  return (
    <InspectionDialog title={title} onClose={close}>
      {inspectItem(inspection.item, inspection.kind)}
    </InspectionDialog>
  );
}

export function parentLocation(
  opened: Opened,
  knownVirtualRoot: boolean,
): ParentLocation | undefined {
  const { rootKind, location, breadcrumbs, page } = opened;
  if (!location || location.resultVersion !== page.resultVersion)
    return undefined;
  if (rootKind === "scope_root") return knownVirtualRoot ? {} : undefined;
  if (rootKind !== "folder") return undefined;
  const current = breadcrumbs.at(-1)?.item;
  const parent = breadcrumbs.at(-2)?.item;
  if (!current || rowKey(current) !== rowKey(location)) return undefined;
  return parent?.resultVersion === page.resultVersion
    ? { item: parent }
    : undefined;
}

export function isPreviousResult(
  displayed?: string,
  selected?: string,
): boolean {
  return (
    displayed !== undefined && selected !== undefined && displayed !== selected
  );
}

export function readOnlyBreadcrumbs(
  entries: ReturnType<typeof folderPath>,
  readOnly: boolean,
) {
  if (!readOnly) return entries;
  return entries.map((entry) => ({
    id: entry.id,
    label: entry.label,
    ...(entry.current === undefined ? {} : { current: entry.current }),
  }));
}

export function PreviousResultNotice({ previous }: { previous: boolean }) {
  const text = useLocaleText();
  if (!previous) return null;
  return (
    <Notice>
      {text(
        "ui.browse.previous_result",
        "Showing the previous result. Open the selected result to navigate.",
      )}
    </Notice>
  );
}

/** The location hierarchy on one line; the current location is drawn as a name. */
function LocationHierarchy({ labels }: { labels: readonly string[] }) {
  const current = labels.at(-1);
  return (
    <>
      {labels
        .slice(0, -1)
        .map((label) => `${label} / `)
        .join("")}
      {current !== undefined && <NameText name={current} />}
    </>
  );
}

/** The origin, observation time, count and details of the folder being shown. */
export function ObservationSummary({
  opened,
  hasVirtualRoot,
  rootLabel,
}: {
  opened: Opened;
  hasVirtualRoot: boolean;
  rootLabel: string | undefined;
}) {
  const text = useLocaleText();
  const regional = useRegionalFormat();
  const timestamp = useTimestampText();
  const page = opened.page;
  // The observation time reads in the app's date and time format, to the minute
  // above the table and in full in the details; a value that cannot be read is
  // unavailable, not the engine's raw text.
  const observed = page.observedAt;
  const observedMillis = observed === null ? Number.NaN : Date.parse(observed);
  const unavailable = text("ui.browse.unavailable", "Unavailable");
  const readable = observed !== null && Number.isFinite(observedMillis);
  // Without a regional setting the time reads in UTC, as ISO 8601 without the T.
  const utc = readable ? observed.slice(0, 19).replace("T", " ") : "";
  const observedShort = !readable
    ? unavailable
    : ((regional && displayMinute(observedMillis, regional)) ??
      utc.slice(0, 16));
  const observedFull = !readable
    ? unavailable
    : timestamp(observedMillis, () => `${utc} UTC`);
  return (
    <div className="du-browse-observation">
      <Text kind="muted">
        {originText(opened.origin, text)} · {observedShort} ·{" "}
        {page.total.state === "lower_bound"
          ? text("ui.browse.item_count_at_least", "At least {count} items", {
              count: String(page.total.count),
            })
          : text("ui.browse.item_count", "{count} items", {
              count: String(page.total.count),
            })}
        {page.completeness === "complete"
          ? ` · ${text("ui.browse.complete", "Complete")}`
          : ""}
      </Text>
      <DetailDisclosure
        title={text("ui.browse.observation_details", "Observation details")}
      >
        <section
          tabIndex={0}
          aria-label={text(
            "ui.browse.full_location_observation",
            "Full folder location and observation",
          )}
        >
          <Readout
            label={text(
              "ui.browse.folder_location_observation",
              "Folder location and observation",
            )}
            rows={[
              {
                label: text(
                  "ui.browse.location_hierarchy",
                  "Location hierarchy",
                ),
                value: (
                  <LocationHierarchy
                    labels={folderPath(
                      opened,
                      hasVirtualRoot,
                      () => undefined,
                      text,
                      rootLabel,
                    ).map((entry) => entry.label)}
                  />
                ),
              },
              {
                label: text("ui.browse.source", "Source"),
                value: originText(opened.origin, text),
              },
              {
                label: text("ui.browse.observed", "Observed"),
                value: observedFull,
              },
              {
                label: text("ui.browse.result", "Result"),
                value: text("ui.browse.pinned_result", "Pinned result"),
              },
              {
                label: text("ui.browse.completeness", "Completeness"),
                value:
                  page.completeness === "complete"
                    ? text("ui.browse.complete", "Complete")
                    : text("ui.browse.incomplete", "Incomplete"),
              },
            ]}
          />
          {page.unknownCount > 0 && (
            <Text kind="muted">
              {text(
                "ui.browse.unknown_items",
                "{count} items could not be evaluated.",
                { count: String(page.unknownCount) },
              )}
            </Text>
          )}
          {page.completeness !== "complete" && (
            <Text>
              {text(
                "ui.browse.some_items_unavailable",
                "Some items may be unavailable.",
              )}
            </Text>
          )}
        </section>
      </DetailDisclosure>
    </div>
  );
}
