import { useEffect, useState } from "react";
import {
  DetailDisclosure,
  ExampleBadge,
  Notice,
  Progress,
  Readout,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import { AnalysisError, countedZeroOf } from "../../ipc";
import { isSampleSource } from "../../state/sample-session";
import type {
  AnalysisClient,
  BrowseValue,
  SpaceDetail,
  SpaceDetailSize,
} from "../../ipc";
import {
  ALLOCATED_MAY_INCLUDE_SHARED,
  displayEvidence,
  displayMeasurementLimit,
  isZeroLowerBound,
  displaySizeWithBytes,
  displaySpaceReason,
} from "./space-format";
import type { SizeDisplay } from "./space-format";

type LocaleText = ReturnType<typeof useLocaleText>;
type Message = readonly [key: string, fallback: string];

function describe<T>(
  field: BrowseValue<T>,
  present: (value: T) => string,
  text: LocaleText,
) {
  switch (field.state) {
    case "present":
      return present(field.value);
    case "absent":
      return text("ui.analysis.space_not_applicable", "Not applicable");
    case "unknown":
      return (
        displaySpaceReason(field.reasonKey, text) ??
        text("ui.inspection.unavailable_in_scan", "Unavailable in this scan")
      );
  }
}

function sizeText(
  value: SpaceDetailSize,
  display: SizeDisplay,
  text: LocaleText,
  countedZero: boolean,
): string {
  if (isZeroLowerBound(value.size) && !countedZero)
    return text(
      "ui.inspection.unavailable_in_scan",
      "Unavailable in this scan",
    );
  return [
    displaySizeWithBytes(value.size, display, text),
    displayEvidence(value.size.evidence, text),
    ...value.measurementLimits
      .filter((key) => key !== ALLOCATED_MAY_INCLUDE_SHARED)
      .map((key) => displayMeasurementLimit(key, text)),
  ]
    .filter(Boolean)
    .join(" · ");
}

function rows(
  detail: SpaceDetail,
  display: SizeDisplay,
  text: LocaleText,
  timestamp: ReturnType<typeof useTimestampText>,
  countedZero: boolean,
) {
  const decision = detail.exclusiveDecision;
  const formatSize = (value: SpaceDetailSize) =>
    sizeText(value, display, text, countedZero);
  const allocatedMayIncludeShared =
    detail.allocated.state === "present" &&
    detail.allocated.value.measurementLimits.includes(
      ALLOCATED_MAY_INCLUDE_SHARED,
    );
  return [
    {
      label: text(
        "ui.analysis.space_observed_logical",
        "Observed logical size",
      ),
      value: describe(detail.logical, formatSize, text),
    },
    {
      label: text(
        "ui.analysis.space_observed_allocated",
        "Observed allocated size",
      ),
      value: describe(detail.allocated, formatSize, text),
    },
    {
      label: text("ui.analysis.space_unique_physical", "Unique physical size"),
      value: describe(detail.uniquePhysical, formatSize, text),
    },
    {
      label: text("ui.analysis.space_exclusive", "Exclusive space"),
      value: describe(detail.exclusive, formatSize, text),
    },
    {
      label: text("ui.analysis.space_shared", "Shared space"),
      value: allocatedMayIncludeShared
        ? text("ui.analysis.shared_space_counted_per_file", "Counted per file")
        : describe(detail.shared, formatSize, text),
      ...(allocatedMayIncludeShared
        ? {
            notes: [
              displayMeasurementLimit(ALLOCATED_MAY_INCLUDE_SHARED, text),
            ],
          }
        : {}),
    },
    {
      label: text(
        "ui.analysis.space_exclusive_confirmed",
        "Exclusive space confirmed",
      ),
      value: decision.confirmed
        ? text("ui.analysis.space_confirmed", "Confirmed")
        : (displaySpaceReason(decision.reasonKey ?? "", text) ??
          text("ui.analysis.space_not_confirmed", "Not confirmed")),
    },
    {
      label: text("ui.analysis.space_file_links", "File links"),
      value: describe(
        detail.linkGroup,
        (group) => {
          const total =
            group.reportedLinkCount === null
              ? text("ui.analysis.space_total_unavailable", "total unavailable")
              : text(
                  "ui.analysis.space_link_count_reported",
                  "{count} reported",
                  {
                    count: BigInt(group.reportedLinkCount),
                  },
                );
          const outside =
            group.hasOutsideLinks === null
              ? text(
                  "ui.analysis.space_outside_links_unknown",
                  "outside links unknown",
                )
              : group.hasOutsideLinks
                ? text(
                    "ui.analysis.space_outside_links_reported",
                    "outside links reported",
                  )
                : text(
                    "ui.analysis.space_no_outside_links_reported",
                    "no outside links reported",
                  );
          return text(
            "ui.analysis.space_links_summary",
            "{count} in scope · {total} · {outside}",
            {
              count: BigInt(group.inScopePaths.length),
              total,
              outside,
            },
          );
        },
        text,
      ),
    },
    {
      label: text("ui.analysis.space_auxiliary", "Auxiliary space"),
      value: describe(
        detail.auxiliary,
        (auxiliary) => {
          const inclusion =
            auxiliary.includedInAllocated === null
              ? text(
                  "ui.analysis.space_allocation_inclusion_unknown",
                  "allocation inclusion unknown",
                )
              : auxiliary.includedInAllocated
                ? text(
                    "ui.analysis.space_included_in_allocation",
                    "included in allocation",
                  )
                : text(
                    "ui.analysis.space_separate_from_allocation",
                    "separate from allocation",
                  );
          return [
            displaySizeWithBytes(auxiliary.size, display, text),
            displayEvidence(auxiliary.size.evidence, text),
            inclusion,
          ]
            .filter(Boolean)
            .join(" · ");
        },
        text,
      ),
    },
    {
      label: text("ui.analysis.space_item_observed_at", "Item observed at"),
      value: describe(
        detail.observedAt,
        (value) => timestamp(Date.parse(value), () => value),
        text,
      ),
    },
    {
      label: text("ui.analysis.space_capabilities", "Space capabilities"),
      value: (
        <>
          {describe(
            detail.capabilities,
            // No capability has a name yet, and its key and state are not shown.
            (capabilities) =>
              capabilities.length > 0
                ? text("ui.common.unknown", "Unknown")
                : text("ui.analysis.space_none_reported", "None reported"),
            text,
          )}
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
    ...(detail.placeholderLocalState
      ? [
          {
            label: text(
              "ui.analysis.space_placeholder_residency",
              "Placeholder residency",
            ),
            // No residency state has a name yet, and the value is not shown.
            value: describe(
              detail.placeholderLocalState,
              () => text("ui.common.unknown", "Unknown"),
              text,
            ),
          },
        ]
      : []),
  ];
}

/** Loads space facts separately so a failure cannot hide the folder attributes. */
export function ItemSpaceDetails({
  client,
  item,
  display,
  onStale,
}: {
  client: AnalysisClient;
  item: SpaceDetail["item"];
  display: SizeDisplay;
  /** Told when the read was refused because the item's result was replaced. */
  onStale?: ((resultVersion: string) => void) | undefined;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const [detail, setDetail] = useState<SpaceDetail>();
  const [problem, setProblem] = useState<Message>();
  const { resultVersion, itemNo } = item;

  useEffect(() => {
    let active = true;
    void client
      .call("space.detail", {
        resultVersion,
        item: { resultVersion, itemNo },
      })
      .then((response) => {
        if (!active) return;
        if (
          response.item.resultVersion !== resultVersion ||
          response.item.itemNo !== itemNo
        ) {
          setProblem([
            "ui.analysis.space_details_mismatch",
            "The space details do not match this item.",
          ]);
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        const stale =
          failure instanceof AnalysisError && failure.code === "STALE_VERSION";
        setProblem(
          stale
            ? [
                "ui.analysis.space_open_latest",
                "Open the latest result to see its space details.",
              ]
            : [
                "ui.analysis.space_details_unavailable",
                "Space details are unavailable.",
              ],
        );
        if (stale) onStale?.(resultVersion);
      });
    return () => {
      active = false;
    };
    // onStale only reports; a new callback does not call for another read.
  }, [client, resultVersion, itemNo]);

  if (problem) return <Notice tone="error">{text(...problem)}</Notice>;
  if (!detail)
    return (
      <Progress
        label={text(
          "ui.analysis.reading_item_space_details",
          "Reading item space details",
        )}
      />
    );
  return (
    <DetailDisclosure
      title={text(
        "ui.analysis.space_measurement_details",
        "Space measurement details",
      )}
    >
      <Readout
        label={text("ui.analysis.observed_item_space", "Observed item space")}
        rows={rows(
          detail,
          display,
          text,
          timestamp,
          countedZeroOf(detail.folderFlags),
        )}
      />
    </DetailDisclosure>
  );
}
