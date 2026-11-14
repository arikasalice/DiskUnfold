import type { LocaleText } from "../../i18n/presentation";
import type { SampleProblem } from "../../state/sample-session";

/**
 * Fallback wording per sample unavailability reason; unknown keys use the
 * generic wording. A sample missing from the installation reads as a full
 * notice of its own; the others complete the sentence "The sample cannot be
 * opened · ...".
 */
export function unavailableText(
  reason: string | undefined,
  text: LocaleText,
): string {
  switch (reason) {
    case "sample.missing":
      return text(
        "ui.sample.reason_missing",
        "This installation does not include sample data. Choose a folder to start analyzing right away.",
      );
    case "sample.format_unsupported":
      return text(
        "ui.sample.reason_format_unsupported",
        "This version cannot read the example data format.",
      );
    case "sample.bundle_invalid":
      return text(
        "ui.sample.reason_bundle_invalid",
        "The example data is damaged and cannot be used.",
      );
    case "sample.load_failed":
      return text(
        "ui.sample.reason_load_failed",
        "The example data could not be read.",
      );
    default:
      return text(
        "ui.sample.reason_generic",
        "The example data cannot be used right now.",
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
