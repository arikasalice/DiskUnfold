import type { LocaleText } from "../../i18n/presentation";
import type { SampleProblem } from "../../state/sample-session";

/** Fallback wording per sample unavailability reason; unknown keys use the generic wording. */
export function unavailableText(
  reason: string | undefined,
  text: LocaleText,
): string {
  switch (reason) {
    case "sample.missing":
      return text(
        "ui.sample.reason_missing",
        "The sample data is missing from this installation.",
      );
    case "sample.format_unsupported":
      return text(
        "ui.sample.reason_format_unsupported",
        "This version cannot read the sample data format.",
      );
    case "sample.bundle_invalid":
      return text(
        "ui.sample.reason_bundle_invalid",
        "The sample data is damaged and cannot be used.",
      );
    case "sample.load_failed":
      return text(
        "ui.sample.reason_load_failed",
        "The sample data could not be loaded.",
      );
    default:
      return text(
        "ui.sample.reason_generic",
        "The sample experience cannot be used right now.",
      );
  }
}

/** Wording for a failed sample command. */
export function problemText(problem: SampleProblem, text: LocaleText): string {
  switch (problem) {
    case "busy":
      return text(
        "ui.sample.busy",
        "The sample experience is busy. Try again in a moment.",
      );
    case "closed":
      return text(
        "ui.sample.closed",
        "The sample experience has already ended. Choose what to analyze to continue.",
      );
    default:
      return text(
        "ui.sample.failed",
        "The sample experience request failed. Try again.",
      );
  }
}
