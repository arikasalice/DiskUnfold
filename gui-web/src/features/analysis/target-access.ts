import type { LocaleText } from "../../i18n/presentation";
import type {
  AnalysisTarget,
  OsPath,
  ScopeReview,
  TargetProbeEvent,
} from "../../ipc";

type ScopeCorrespondence = ScopeReview["targets"][number];
type TargetAccess = AnalysisTarget["access"];

/** The reason of a network volume the probe deliberately did not open (F01 #266). */
export const ACCESS_NOT_CHECKED = "target.access_not_checked";

/** A target's access as its card shows it: a network volume may wait for selection. */
export type ShownAccess = TargetAccess | "not_checked";

/**
 * The default list holds what the OS file manager shows (F01 #263). A hidden target
 * stays listed while it is the selected one, and no visibility evidence means shown.
 */
export function defaultListing(
  entries: readonly AnalysisTarget[],
  selected: OsPath | undefined,
  showHidden: boolean,
): { listed: readonly AnalysisTarget[]; hidden: number } {
  const inDefault = (target: AnalysisTarget) =>
    target.defaultVisible !== false || target.path.raw === selected?.raw;
  return {
    listed: showHidden ? entries : entries.filter(inDefault),
    hidden: entries.filter((target) => !inDefault(target)).length,
  };
}

/**
 * The access a target card shows, and whether its check waits for selection.
 *
 * A network volume is not opened before it is selected, so the scope review that a
 * selection starts is its check (F01 #266): it reads as checking while that review
 * runs and then shows the review's result for the selected path.
 */
export function shownAccess(
  target: AnalysisTarget,
  probe: TargetProbeEvent | undefined,
  finished: boolean,
  selection: {
    selected: boolean;
    reviewing: boolean;
    reviewed: readonly ScopeCorrespondence[] | undefined;
  },
): { access: ShownAccess; deferred: boolean } {
  const access =
    probe?.access ??
    (finished && target.access === "checking" ? "unknown" : target.access);
  const reason = probe ? probe.reasonKey : target.reasonKey;
  if (access !== "unknown" || reason !== ACCESS_NOT_CHECKED)
    return { access, deferred: false };
  if (!selection.selected) return { access: "not_checked", deferred: true };
  if (selection.reviewing) return { access: "checking", deferred: true };
  const reviewed = selection.reviewed?.find(
    ({ path }) => path.raw === target.path.raw,
  );
  return { access: reviewed?.access ?? "not_checked", deferred: true };
}

/** The status text of a target card. */
export function accessLabel(access: ShownAccess, text: LocaleText): string {
  switch (access) {
    case "checking":
      return text("ui.analysis.target_access_checking", "checking");
    case "accessible":
      return text("ui.analysis.target_access_accessible", "accessible");
    case "denied":
      return text("ui.analysis.target_access_denied", "denied");
    case "disconnected":
      return text("ui.analysis.target_access_disconnected", "disconnected");
    case "missing":
      return text("ui.analysis.target_access_missing", "missing");
    case "unresponsive":
      return text("ui.analysis.target_access_unresponsive", "unresponsive");
    case "not_checked":
      return text(
        "ui.analysis.target_access_not_checked",
        "Checked when selected",
      );
    default:
      return text("ui.analysis.target_access_unknown", "unknown");
  }
}
