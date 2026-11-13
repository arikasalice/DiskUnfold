import { Notice, Readout, StatusBadge } from "../../components";
import type { SupportCombination } from "../../ipc";
import { cpuLabel, osLabel, statusLabel } from "./support-copy";
import type { SupportCopy } from "./support-copy";

function statusTone(status: string) {
  if (status === "officially_verified") return "good" as const;
  if (status === "conditional") return "warning" as const;
  if (status === "unsupported_found") return "danger" as const;
  if (status === "not_verified") return "info" as const;
  return "neutral" as const;
}

/**
 * The embedded support table, in its published order (F15 IF-04). Each
 * combination is one row that wraps, so a long system name or configuration
 * list is never cut off and the dialog needs no sideways scrolling.
 */
export function CombinationTable({
  combinations,
  copy,
}: {
  combinations: readonly SupportCombination[];
  copy: SupportCopy;
}) {
  if (combinations.length === 0) {
    return <Notice tone="info">{copy.combinationsNone}</Notice>;
  }
  return (
    <div className="du-info-areas">
      <Readout
        label={copy.combinationsHeading}
        rows={combinations.map((row) => ({
          label: copy.systemValue(
            osLabel(row.identity.osFamily, copy),
            row.identity.release ?? copy.notSpecified,
            row.identity.edition ?? copy.notSpecified,
            cpuLabel(row.identity.cpu, copy),
          ),
          value: (
            <StatusBadge tone={statusTone(row.status)}>
              {statusLabel(row.status, copy)}
            </StatusBadge>
          ),
          notes: [
            copy.detailsValue(
              row.verifiedDetails.length > 0
                ? row.verifiedDetails.join(", ")
                : copy.notSpecified,
            ),
          ],
        }))}
      />
    </div>
  );
}
