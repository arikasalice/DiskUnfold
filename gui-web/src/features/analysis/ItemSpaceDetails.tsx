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
import { AnalysisError } from "../../ipc";
import { isSampleSource } from "../../state/sample-session";
import type {
  AnalysisClient,
  BrowseValue,
  SpaceDetail,
  SpaceDetailSize,
} from "../../ipc";
import {
  displayEvidence,
  displayMeasurementLimit,
  displaySizeWithBytes,
} from "./space-format";
import type { SizeDisplay } from "./space-format";

type LocaleText = ReturnType<typeof useLocaleText>;
type Message = readonly [key: string, fallback: string];

function spaceReason(key: string, text: LocaleText): string | undefined {
  switch (key) {
    case "space.aux_not_included":
      return text(
        "ui.analysis.space_aux_not_included",
        "Auxiliary data was not collected",
      );
    case "space.dedup_capacity":
      return text(
        "ui.analysis.space_dedup_capacity",
        "The file identity limit was reached",
      );
    case "space.sharing_not_collected":
      return text(
        "ui.analysis.space_sharing_not_collected",
        "Sharing evidence was not collected",
      );
    case "space.sharing_unknown":
      return text(
        "ui.analysis.space_sharing_unknown",
        "Sharing could not be verified",
      );
    case "space.identity_unsupported":
      return text(
        "ui.analysis.space_identity_unsupported",
        "File identity could not be verified",
      );
    case "space.out_of_range_links":
      return text(
        "ui.analysis.space_out_of_range_links",
        "Links outside the scan were reported",
      );
    case "space.shared_reported":
      return text(
        "ui.analysis.space_shared_reported",
        "The file system reported shared space",
      );
    default:
      return undefined;
  }
}

function measurementLimit(key: string, text: LocaleText): string {
  switch (key) {
    case "space.data_insufficient":
      return text(
        "ui.analysis.space_data_insufficient",
        "Some space data could not be verified",
      );
    case "space.aggregation_overflow":
      return text(
        "ui.analysis.space_aggregation_overflow",
        "The space total exceeded the supported range",
      );
    case "space.allocated_not_collected":
      return text(
        "ui.analysis.space_allocated_not_collected",
        "Allocation data was not collected",
      );
    case "space.observation_detail_insufficient":
      return text(
        "ui.analysis.space_observation_detail_insufficient",
        "Some observation details are unavailable",
      );
    default:
      return (
        spaceReason(key, text) ??
        text(
          "ui.analysis.space_measurement_limit_reported",
          displayMeasurementLimit(key),
        )
      );
  }
}

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
        spaceReason(field.reasonKey, text) ??
        text("ui.inspection.unavailable_in_scan", "Unavailable in this scan")
      );
  }
}

function sizeText(
  value: SpaceDetailSize,
  display: SizeDisplay,
  text: LocaleText,
): string {
  return [
    displaySizeWithBytes(value.size, display, text),
    displayEvidence(value.size.evidence, text),
    ...value.measurementLimits.map((key) => measurementLimit(key, text)),
  ]
    .filter(Boolean)
    .join(" · ");
}

function rows(
  detail: SpaceDetail,
  display: SizeDisplay,
  text: LocaleText,
  timestamp: ReturnType<typeof useTimestampText>,
) {
  const decision = detail.exclusiveDecision;
  const formatSize = (value: SpaceDetailSize) => sizeText(value, display, text);
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
      label: text("ui.analysis.space_unique_physical", "Unique physical space"),
      value: describe(detail.uniquePhysical, formatSize, text),
    },
    {
      label: text("ui.analysis.space_exclusive", "Exclusive space"),
      value: describe(detail.exclusive, formatSize, text),
    },
    {
      label: text("ui.analysis.space_shared", "Shared space"),
      value: describe(detail.shared, formatSize, text),
    },
    {
      label: text(
        "ui.analysis.space_exclusive_confirmed",
        "Exclusive space confirmed",
      ),
      value: decision.confirmed
        ? text("ui.analysis.space_confirmed", "Confirmed")
        : (spaceReason(decision.reasonKey ?? "", text) ??
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
            (capabilities) =>
              capabilities
                .map(({ key, state }) => `${key} · ${state}`)
                .join(", ") ||
              text("ui.analysis.space_none_reported", "None reported"),
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
            value: describe(detail.placeholderLocalState, String, text),
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
}: {
  client: AnalysisClient;
  item: SpaceDetail["item"];
  display: SizeDisplay;
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
        setProblem(
          failure instanceof AnalysisError && failure.code === "STALE_VERSION"
            ? [
                "ui.analysis.space_open_latest",
                "Open the latest result to see its space details.",
              ]
            : [
                "ui.analysis.space_details_unavailable",
                "Space details are unavailable.",
              ],
        );
      });
    return () => {
      active = false;
    };
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
        rows={rows(detail, display, text, timestamp)}
      />
    </DetailDisclosure>
  );
}
