import type {
  StartupAccessState,
  StartupCheckResult,
  StartupGrade,
} from "../../ipc";
import type { LocaleText } from "../../i18n/presentation";

export function startupCopy(text: LocaleText) {
  return {
    title: text("ui.startup.title", "Startup and privacy"),
    introduction: text(
      "ui.startup.introduction",
      "Review startup checks and Full Disk Access for this Mac. Disk analysis is not enabled in this build.",
    ),
    loading: text("ui.startup.loading", "Loading startup checks"),
    loadFailed: text(
      "ui.startup.load_failed",
      "Startup status could not be read. Try again.",
    ),
    actionFailed: text(
      "ui.startup.action_failed",
      "The permission status could not be updated. Try again.",
    ),
    retry: text("ui.startup.retry", "Try again"),
    statusHeading: text("ui.startup.status_heading", "Startup status"),
    checksHeading: text("ui.startup.checks_heading", "Checks"),
    grade: text("ui.startup.grade", "Overall status"),
    access: text("ui.startup.access", "Full Disk Access"),
    analysis: text("ui.startup.analysis", "Analysis entry"),
    allowed: text("ui.startup.allowed", "Allowed by startup checks"),
    blocked: text("ui.startup.blocked", "Blocked by startup checks"),
    deniedNotice: text(
      "ui.startup.denied_notice",
      "Full Disk Access is denied. Open Privacy & Security Settings, add DiskUnfold under Full Disk Access, then check the permission again.",
    ),
    analysisBlockedNotice: text(
      "ui.startup.analysis_blocked_notice",
      "Startup checks do not allow analysis. Review the listed results for details.",
    ),
    unknownNotice: text(
      "ui.startup.unknown_notice",
      "macOS could not confirm Full Disk Access. Unknown access is not treated as a denial. You can open Privacy & Security Settings and check again.",
    ),
    confirmedNotice: text(
      "ui.startup.confirmed_notice",
      "Full Disk Access is confirmed. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    ),
    notApplicableNotice: text(
      "ui.startup.not_applicable_notice",
      "Full Disk Access does not apply to this operation. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    ),
    openSettings: text(
      "ui.startup.open_settings",
      "Open Privacy & Security Settings",
    ),
    recheck: text("ui.startup.recheck", "Check permission again"),
    settingsOpened: text(
      "ui.startup.settings_opened",
      "System Settings reported that its privacy page opened. Grant Full Disk Access to DiskUnfold, then check the permission again.",
    ),
    settingsNotConfirmed: text(
      "ui.startup.settings_not_confirmed",
      "macOS did not confirm that Settings opened. If needed, open System Settings › Privacy & Security › Full Disk Access, add DiskUnfold, then check again.",
    ),
    rechecked: text("ui.startup.rechecked", "Permission status refreshed."),
    unknown: text("ui.startup.unknown", "Unknown"),
    pass: text("ui.startup.pass", "Passed"),
    limited: text("ui.startup.limited", "Limited"),
    outsideVerified: text(
      "ui.startup.outside_verified",
      "Outside verified combinations",
    ),
    unverifiable: text("ui.startup.unverifiable", "Could not verify"),
    notApplicable: text("ui.startup.not_applicable", "Not applicable"),
    denied: text("ui.startup.denied", "Denied"),
    confirmed: text("ui.startup.confirmed", "Confirmed"),
    bannerHeading: text("ui.startup.banner_heading", "Startup notice"),
    bannerLimited: text(
      "ui.startup.banner_limited",
      "Some DiskUnfold features are limited on this system.",
    ),
    bannerOutsideVerified: text(
      "ui.startup.banner_outside_verified",
      "This system is not one of the combinations DiskUnfold has verified. It can still run, but some behavior may differ.",
    ),
    bannerUnverifiable: text(
      "ui.startup.banner_unverifiable",
      "Some startup checks could not be verified. DiskUnfold keeps running.",
    ),
    bannerUnknown: text(
      "ui.startup.banner_unknown",
      "Startup checks reported a status this version does not recognize.",
    ),
    bannerDismiss: text("ui.startup.banner_dismiss", "Dismiss"),
    checkNames: {
      resource_integrity: text(
        "ui.startup.check.resource_integrity",
        "Built-in resources",
      ),
      webview: text("ui.startup.check.webview", "WebView availability"),
      data_format_and_open: text(
        "ui.startup.check.data_format_and_open",
        "Local data and format",
      ),
      temporary_files: text(
        "ui.startup.check.temporary_files",
        "Temporary files",
      ),
      support_matrix: text(
        "ui.startup.check.support_matrix",
        "Verified environment",
      ),
      capability: text("ui.startup.check.capability", "Platform capabilities"),
      full_disk_access: text(
        "ui.startup.check.full_disk_access",
        "Full Disk Access",
      ),
    } as Record<string, string>,
  };
}

export type StartupCopy = ReturnType<typeof startupCopy>;

export function gradeLabel(grade: StartupGrade, copy: StartupCopy) {
  switch (grade) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    default:
      return copy.unknown;
  }
}

export function accessLabel(access: StartupAccessState, copy: StartupCopy) {
  switch (access) {
    case "confirmed":
      return copy.confirmed;
    case "denied":
      return copy.denied;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

export function outcomeLabel(result: StartupCheckResult, copy: StartupCopy) {
  switch (result.result) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

export function checkLabel(check: StartupCheckResult, copy: StartupCopy) {
  return copy.checkNames[check.check] ?? copy.unknown;
}
