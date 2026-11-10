import { useEffect, useState } from "react";
import { Notice, Progress, Readout } from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  BrowseValue,
  SpaceDetail,
  SpaceDetailSize,
} from "../../ipc";
import { displaySize, evidenceLabels } from "./space-format";
import type { SizeDisplay } from "./space-format";

const reasonText: Readonly<Record<string, string>> = {
  "space.aux_not_included": "Auxiliary data was not collected",
  "space.dedup_capacity": "The file identity limit was reached",
  "space.sharing_not_collected": "Sharing evidence was not collected",
  "space.sharing_unknown": "Sharing could not be verified",
  "space.identity_unsupported": "File identity could not be verified",
  "space.out_of_range_links": "Links outside the scan were reported",
  "space.shared_reported": "The file system reported shared space",
};

function describe<T>(field: BrowseValue<T>, present: (value: T) => string) {
  switch (field.state) {
    case "present":
      return present(field.value);
    case "absent":
      return "Not applicable";
    case "unknown":
      return reasonText[field.reasonKey] ?? "Unavailable in this scan";
  }
}

function sizeText(value: SpaceDetailSize, display: SizeDisplay): string {
  return [
    displaySize(value.size, display),
    evidenceLabels[value.size.evidence],
    ...value.measurementLimits.map((key) => reasonText[key]).filter(Boolean),
  ]
    .filter(Boolean)
    .join(" · ");
}

function rows(detail: SpaceDetail, display: SizeDisplay) {
  const decision = detail.exclusiveDecision;
  const formatSize = (value: SpaceDetailSize) => sizeText(value, display);
  return [
    {
      label: "Observed logical size",
      value: describe(detail.logical, formatSize),
    },
    {
      label: "Observed allocated size",
      value: describe(detail.allocated, formatSize),
    },
    {
      label: "Unique physical space",
      value: describe(detail.uniquePhysical, formatSize),
    },
    { label: "Exclusive space", value: describe(detail.exclusive, formatSize) },
    { label: "Shared space", value: describe(detail.shared, formatSize) },
    {
      label: "Exclusive space confirmed",
      value: decision.confirmed
        ? "Confirmed"
        : (reasonText[decision.reasonKey ?? ""] ?? "Not confirmed"),
    },
    {
      label: "File links",
      value: describe(detail.linkGroup, (group) => {
        const total =
          group.reportedLinkCount === null
            ? "total unavailable"
            : `${BigInt(group.reportedLinkCount).toLocaleString()} reported`;
        const outside =
          group.hasOutsideLinks === null
            ? "outside links unknown"
            : group.hasOutsideLinks
              ? "outside links reported"
              : "no outside links reported";
        return `${group.inScopePaths.length.toLocaleString()} in scope · ${total} · ${outside}`;
      }),
    },
    {
      label: "Auxiliary space",
      value: describe(detail.auxiliary, (auxiliary) => {
        const inclusion =
          auxiliary.includedInAllocated === null
            ? "allocation inclusion unknown"
            : auxiliary.includedInAllocated
              ? "included in allocation"
              : "separate from allocation";
        return `${displaySize(auxiliary.size, display)} · ${inclusion}`;
      }),
    },
    { label: "Item observed at", value: describe(detail.observedAt, String) },
    {
      label: "Space capabilities",
      value: describe(
        detail.capabilities,
        (capabilities) =>
          capabilities
            .map(({ key, state }) => `${key} · ${state}`)
            .join(", ") || "None reported",
      ),
    },
    ...(detail.placeholderLocalState
      ? [
          {
            label: "Placeholder residency",
            value: describe(detail.placeholderLocalState, String),
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
  const [detail, setDetail] = useState<SpaceDetail>();
  const [problem, setProblem] = useState<string>();
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
          setProblem("The space details do not match this item.");
          return;
        }
        setDetail(response);
      })
      .catch((failure: unknown) => {
        if (!active) return;
        setProblem(
          failure instanceof AnalysisError && failure.code === "STALE_VERSION"
            ? "Open the latest result to see its space details."
            : "Space details are unavailable.",
        );
      });
    return () => {
      active = false;
    };
  }, [client, resultVersion, itemNo]);

  if (problem) return <Notice tone="error">{problem}</Notice>;
  if (!detail) return <Progress label="Reading item space details" />;
  return <Readout label="Observed item space" rows={rows(detail, display)} />;
}
