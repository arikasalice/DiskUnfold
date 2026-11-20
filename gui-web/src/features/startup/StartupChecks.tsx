import { useMemo } from "react";
import {
  Action,
  DetailDisclosure,
  Readout,
  ReviewSection,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { StartupCheckResult, StartupStatus } from "../../ipc";
import {
  checkLabel,
  checkRows,
  outcomeLabel,
  startupCopy,
} from "./startup-copy";
import type { StartupCopy } from "./startup-copy";
import { useStartupMemory } from "./startup-memory";

function overallLabel(status: StartupStatus, copy: StartupCopy) {
  switch (status.grade) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.overallLimited;
    case "outside_verified":
      return copy.overallOutsideVerified;
    default:
      return copy.unverifiable;
  }
}

/**
 * One line under a row that did not pass: what the user can do, or that
 * nothing is wrong when there is nothing to do. It sums the row up; the long
 * explanation stays once in the sections below (design F15 README 1.6).
 */
function description(row: StartupCheckResult, copy: StartupCopy) {
  if (row.check === "full_disk_access") return copy.noticeFullDiskAccess;
  if (row.check === "support_matrix") {
    return row.result === "outside_verified"
      ? copy.describeUntested
      : copy.describeTestedUnknown;
  }
  if (row.result === "limited") return copy.describeLimited;
  return copy.describeUnverifiable;
}

/**
 * The startup checks section at the top of support information (F15
 * FR6.HD2.gui-web.DD1; design F15 README 1.3).
 *
 * It shows the overall status, then each row that did not pass with one line
 * of explanation, and every check behind "Show all checks". The results were
 * fixed when the app started, which the section says. `onShowCombination`
 * moves to the combination status section of the same screen. Nothing shows
 * before the startup status has been read.
 */
export function StartupChecks({
  onShowCombination,
}: {
  onShowCombination?: () => void;
}) {
  const text = useLocaleText();
  const copy = useMemo(() => startupCopy(text), [text]);
  const { status } = useStartupMemory();
  if (!status) return null;
  const open = status.checks.filter(
    (row) => row.result !== "pass" && row.result !== "not_applicable",
  );
  return (
    <ReviewSection title={copy.checksSection}>
      <Text kind="muted">{copy.checkedAtStart}</Text>
      <Readout
        label={copy.checksSection}
        rows={[
          { label: copy.grade, value: overallLabel(status, copy) },
          ...open.map((row) => ({
            label: checkLabel(row, copy),
            value: outcomeLabel(row, copy),
            notes: [description(row, copy)],
          })),
        ]}
      />
      {onShowCombination &&
        open.some((row) => row.check === "support_matrix") && (
          <div className="du-info-actions">
            <Action variant="quiet" onPress={onShowCombination}>
              {copy.showCombination}
            </Action>
          </div>
        )}
      <DetailDisclosure title={copy.showAllChecks}>
        <Readout
          label={copy.showAllChecks}
          rows={checkRows(status, copy, status.access !== "not_applicable")}
        />
      </DetailDisclosure>
    </ReviewSection>
  );
}
