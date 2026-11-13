import { DataTable, Notice } from "../../components";
import type { SupportCombination } from "../../ipc";
import { cpuLabel, osLabel, statusLabel } from "./support-copy";
import type { SupportCopy } from "./support-copy";

/** The embedded support table, in its published order (F15 IF-04). */
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
    <DataTable
      label={copy.combinationsHeading}
      density="compact"
      columns={[
        copy.columnSystem,
        copy.columnCpu,
        copy.columnStatus,
        copy.columnDetails,
      ]}
      columnWidths={[240, 90, 190, 300]}
      rows={combinations}
      renderCells={(row) => [
        [
          osLabel(row.identity.osFamily, copy),
          row.identity.release ?? copy.notSpecified,
          row.identity.edition ?? copy.notSpecified,
        ].join(" · "),
        cpuLabel(row.identity.cpu, copy),
        statusLabel(row.status, copy),
        row.verifiedDetails.length > 0
          ? row.verifiedDetails.join(", ")
          : copy.notSpecified,
      ]}
    />
  );
}
