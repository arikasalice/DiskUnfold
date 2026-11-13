import type { LocaleText } from "../../i18n/presentation";
import type {
  SecuritySupportKind,
  SupportCapabilityState,
  SupportCpu,
  SupportOsFamily,
  SupportStatus,
} from "../../ipc";

/** User-facing words for the support information screen (F15 IF-04). */
export function supportCopy(text: LocaleText) {
  return {
    intro: text(
      "ui.support.intro",
      "This compares your computer with the combinations the DiskUnfold project has tested. A combination that was not tested is not necessarily broken.",
    ),
    loading: text("ui.support.loading", "Loading support information"),
    loadFailed: text(
      "ui.support.load_failed",
      "Support information could not be read.",
    ),
    refreshFailed: text(
      "ui.support.refresh_failed",
      "Support information could not be refreshed. The values below are from the last successful read.",
    ),
    retry: text("ui.common.retry", "Retry"),
    refresh: text("ui.support.refresh", "Refresh"),
    tableHeading: text("ui.support.table_heading", "Support table"),
    matrixRevision: text("ui.support.matrix_revision", "Revision"),
    matrixAsOf: text("ui.support.matrix_as_of", "Prepared on"),
    matrixMissing: text("ui.support.matrix_missing", "Not available"),
    matrixUnavailable: text(
      "ui.support.matrix_unavailable",
      "This build has no support table, so DiskUnfold cannot say whether this computer has been tested. That does not mean it is unsupported. Check the support table on the official release page.",
    ),
    envHeading: text("ui.support.env_heading", "This computer"),
    envOs: text("ui.support.env_os", "Operating system"),
    envRelease: text("ui.support.env_release", "Release"),
    envEdition: text("ui.support.env_edition", "Edition"),
    envCpu: text("ui.support.env_cpu", "CPU"),
    notDetected: text("ui.support.not_detected", "Not detected"),
    envUnknown: text(
      "ui.support.env_unknown",
      "DiskUnfold could not identify this computer's operating system or CPU, so it cannot match it against the support table. This is not a sign of a problem. Check your system version in your system settings and compare it with the support table on the official release page.",
    ),
    reasonOther: text(
      "ui.support.reason_other",
      "DiskUnfold reported a reason that this version does not recognize.",
    ),
    combinationHeading: text(
      "ui.support.combination_heading",
      "Combination status",
    ),
    combinationLabel: text("ui.support.combination_label", "Status"),
    securityHeading: text("ui.support.security_heading", "Security support"),
    securityIntro: text(
      "ui.support.security_intro",
      "This is the system vendor's security-update status. It is separate from the combination status above.",
    ),
    securityNone: text(
      "ui.support.security_none",
      "No security-support information is recorded for this system.",
    ),
    securityKind: text("ui.support.security_kind", "Vendor support"),
    securitySource: text("ui.support.security_source", "Source"),
    securityChecked: text("ui.support.security_checked", "Checked on"),
    featuresHeading: text("ui.support.features_heading", "Feature areas"),
    featuresNone: text(
      "ui.support.features_none",
      "No feature-area details were reported.",
    ),
    tableLabel: text("ui.support.table_label", "Support table"),
    nowLabel: text("ui.support.now_label", "Right now"),
    conditionLabel: text("ui.support.condition_label", "Condition"),
    mismatch: text(
      "ui.support.mismatch",
      "This differs from the support table. What is shown as the current state is what this computer does right now.",
    ),
    capabilityLimited: text(
      "ui.support.capability_limited",
      "This feature area is limited on this computer.",
    ),
    combinationsHeading: text(
      "ui.support.combinations_heading",
      "Tested combinations",
    ),
    combinationsNone: text(
      "ui.support.combinations_none",
      "The support table lists no combinations.",
    ),
    columnSystem: text("ui.support.column_system", "System"),
    columnCpu: text("ui.support.column_cpu", "CPU"),
    columnStatus: text("ui.support.column_status", "Status"),
    columnDetails: text("ui.support.column_details", "Tested configurations"),
    notSpecified: text("ui.support.not_specified", "Not specified"),
    statuses: {
      officially_verified: text(
        "ui.support.status_officially_verified",
        "Officially verified",
      ),
      conditional: text(
        "ui.support.status_conditional",
        "Verified with conditions",
      ),
      unsupported_found: text(
        "ui.support.status_unsupported_found",
        "Problems found",
      ),
      not_verified: text("ui.support.status_not_verified", "Not tested"),
      unknown: text("ui.support.status_unknown", "Could not be determined"),
    } as Record<SupportStatus, string>,
    statusNotes: {
      officially_verified: text(
        "ui.support.status_note_officially_verified",
        "The project has tested this exact operating system and CPU combination.",
      ),
      conditional: text(
        "ui.support.status_note_conditional",
        "The project has tested this combination, with the conditions listed under the feature areas.",
      ),
      unsupported_found: text(
        "ui.support.status_note_unsupported_found",
        "Testing found problems on this combination. Some features may not work.",
      ),
      not_verified: text(
        "ui.support.status_note_not_verified",
        "This combination is not in the support table. It has not been tested, which does not mean it fails.",
      ),
      unknown: text(
        "ui.support.status_note_unknown",
        "The status of this combination could not be determined.",
      ),
    } as Record<SupportStatus, string>,
    capabilities: {
      supported: text("ui.support.capability_supported", "Working"),
      limited: text("ui.support.capability_limited_label", "Limited"),
      unsupported: text("ui.support.capability_unsupported", "Not working"),
      not_applicable: text("ui.support.capability_na", "Not applicable"),
      unknown: text("ui.support.capability_unknown", "Could not be determined"),
    } as Record<SupportCapabilityState, string>,
    areas: {
      full_disk_access: text(
        "ui.support.area_full_disk_access",
        "Full Disk Access",
      ),
      webview: text("ui.support.area_webview", "WebView"),
    } as Record<string, string>,
    areaOther: text("ui.support.area_other", "Other feature area"),
    osFamilies: {
      windows: text("ui.support.os_windows", "Windows"),
      macos: text("ui.support.os_macos", "macOS"),
      linux: text("ui.support.os_linux", "Linux"),
      unknown: text("ui.support.os_unknown", "Unknown"),
    } as Record<SupportOsFamily, string>,
    cpus: {
      x64: text("ui.support.cpu_x64", "x64"),
      arm64: text("ui.support.cpu_arm64", "ARM64"),
      other: text("ui.support.cpu_other", "Other"),
      unknown: text("ui.support.cpu_unknown", "Unknown"),
    } as Record<SupportCpu, string>,
    securityKinds: {
      general: text("ui.support.security_general", "Standard support"),
      extended: text("ui.support.security_extended", "Extended support"),
      unknown: text("ui.support.security_unknown", "Not stated"),
    } as Record<SecuritySupportKind, string>,
  };
}

export type SupportCopy = ReturnType<typeof supportCopy>;

function lookup<K extends string>(
  table: Record<K, string>,
  key: string,
  fallback: string,
): string {
  return Object.hasOwn(table, key) ? table[key as K] : fallback;
}

/** Words for a status; an unfamiliar value reads as undetermined. */
export function statusLabel(status: string, copy: SupportCopy): string {
  return lookup(copy.statuses, status, copy.statuses.unknown);
}

/** Plain-language explanation of a combination status. */
export function statusNote(status: string, copy: SupportCopy): string {
  return lookup(copy.statusNotes, status, copy.statusNotes.unknown);
}

/** Words for a current capability; an unfamiliar value reads as undetermined. */
export function capabilityLabel(state: string, copy: SupportCopy): string {
  return lookup(copy.capabilities, state, copy.capabilities.unknown);
}

/** Words for a feature area; an unfamiliar area keeps a generic name. */
export function areaLabel(area: string, copy: SupportCopy): string {
  return lookup(copy.areas, area, copy.areaOther);
}

/** Operating-system words for the environment and combination rows. */
export function osLabel(os: string, copy: SupportCopy): string {
  return lookup(copy.osFamilies, os, copy.osFamilies.unknown);
}

/** CPU words for the environment and combination rows. */
export function cpuLabel(cpu: string, copy: SupportCopy): string {
  return lookup(copy.cpus, cpu, copy.cpus.unknown);
}

/** Vendor security-support words. */
export function securityKindLabel(
  kind: string | null,
  copy: SupportCopy,
): string {
  return kind === null
    ? copy.securityKinds.unknown
    : lookup(copy.securityKinds, kind, copy.securityKinds.unknown);
}

/** Maps a host reason key to guidance; an unfamiliar key gets a generic line. */
export function reasonText(reasonKey: string, copy: SupportCopy): string {
  switch (reasonKey) {
    case "support.env_unknown":
      return copy.envUnknown;
    case "support.matrix_unavailable":
      return copy.matrixUnavailable;
    case "support.capability_limited":
      return copy.capabilityLimited;
    default:
      return copy.reasonOther;
  }
}

/**
 * True when the support table promises more than this computer does now.
 * Only a recorded verified or conditional status can conflict with a limited
 * or unsupported current capability.
 */
export function differsFromTable(status: string, capability: string): boolean {
  return (
    (status === "officially_verified" || status === "conditional") &&
    (capability === "limited" || capability === "unsupported")
  );
}
