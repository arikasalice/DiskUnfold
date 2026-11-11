import { useId } from "react";
import {
  Action,
  DataTable,
  Heading,
  Notice,
  Progress,
  Text,
} from "../../components";
import type { VizRow } from "../../ipc";
import type { VizText } from "./model-text";

export type GroupMembersPresenterProps = {
  /** The label of the group selected in the parent visualization. */
  title?: string;
  /** Caller-selected destination, such as "Back to graph" or "Back to table". */
  returnLabel: string;
  /**
   * Rows from a page already queried and validated by the owner. When these
   * rows are retained during loading or error, `rangeLabel` must describe this
   * same last confirmed page; the controller owns that invariant.
   */
  rows: readonly VizRow[] | undefined;
  /** Display strings are supplied by the owner; absent evidence stays absent. */
  rangeLabel?: string;
  groupTotalLabel?: string;
  sizeBasisLabel?: string;
  sizeRangeLabel?: string;
  observedAtLabel?: string;
  status: "loading" | "ready" | "error";
  errorMessage?: string;
  /** True only for retryable page errors; version/model recovery belongs upstream. */
  canRetry: boolean;
  hasPrevious: boolean;
  hasNext: boolean;
  text: Pick<VizText, "quantity" | "size">;
  onClose: () => void;
  onPrevious: () => void;
  onNext: () => void;
  onRetry: () => void;
};

/** Displays an already supplied group page; it owns no IPC or page state. */
export function GroupMembersPresenter({
  title = "Other members",
  returnLabel,
  rows,
  rangeLabel,
  groupTotalLabel,
  sizeBasisLabel,
  sizeRangeLabel,
  observedAtLabel,
  status,
  errorMessage,
  canRetry,
  hasPrevious,
  hasNext,
  text,
  onClose,
  onPrevious,
  onNext,
  onRetry,
}: GroupMembersPresenterProps) {
  const headingId = useId();
  const loading = status === "loading";

  return (
    <section
      className="du-review-stack du-group-members"
      aria-labelledby={headingId}
    >
      <header className="du-review-actions">
        <Heading level={2} id={headingId}>
          {title}
        </Heading>
        <Action onPress={onClose}>{returnLabel}</Action>
      </header>

      <GroupMeasurementContext
        rangeLabel={rangeLabel}
        groupTotalLabel={groupTotalLabel}
        sizeBasisLabel={sizeBasisLabel}
        sizeRangeLabel={sizeRangeLabel}
        observedAtLabel={observedAtLabel}
      />
      <GroupPageFeedback
        status={status}
        canRetry={canRetry}
        errorMessage={errorMessage}
        hasEmptyPage={isConfirmedEmptyPage(status, rows)}
        onRetry={onRetry}
      />
      <GroupRows rows={rows} text={text} />

      <nav className="du-review-actions" aria-label="Group member ranges">
        <Action isDisabled={loading || !hasPrevious} onPress={onPrevious}>
          Previous range
        </Action>
        <Action isDisabled={loading || !hasNext} onPress={onNext}>
          Next range
        </Action>
      </nav>
    </section>
  );
}

function isConfirmedEmptyPage(
  status: GroupMembersPresenterProps["status"],
  rows: readonly VizRow[] | undefined,
) {
  return status === "ready" && rows !== undefined && rows.length === 0;
}

function GroupMeasurementContext({
  rangeLabel,
  groupTotalLabel,
  sizeBasisLabel,
  sizeRangeLabel,
  observedAtLabel,
}: {
  rangeLabel?: string | undefined;
  groupTotalLabel?: string | undefined;
  sizeBasisLabel?: string | undefined;
  sizeRangeLabel?: string | undefined;
  observedAtLabel?: string | undefined;
}) {
  if (
    !rangeLabel &&
    !groupTotalLabel &&
    !sizeBasisLabel &&
    !sizeRangeLabel &&
    !observedAtLabel
  )
    return null;
  return (
    <div className="du-review-context" aria-label="Group measurement context">
      {rangeLabel && <Text>{rangeLabel}</Text>}
      {groupTotalLabel && <Text>{groupTotalLabel}</Text>}
      {sizeBasisLabel && <Text>{sizeBasisLabel}</Text>}
      {sizeRangeLabel && <Text>{sizeRangeLabel}</Text>}
      {observedAtLabel && <Text>{observedAtLabel}</Text>}
    </div>
  );
}

function GroupPageFeedback({
  status,
  canRetry,
  errorMessage,
  hasEmptyPage,
  onRetry,
}: Pick<GroupMembersPresenterProps, "status" | "canRetry" | "onRetry"> & {
  errorMessage?: string | undefined;
  hasEmptyPage: boolean;
}) {
  return (
    <>
      {status === "loading" && <Progress label="Loading members" />}
      {status === "error" && (
        <Notice tone="error">
          {errorMessage ?? "Could not load this range."}
        </Notice>
      )}
      {status === "error" && canRetry && (
        <div className="du-review-actions">
          <Action onPress={onRetry}>Retry</Action>
        </div>
      )}
      {hasEmptyPage && <Notice>No members in this range.</Notice>}
    </>
  );
}

function GroupRows({
  rows,
  text,
}: {
  rows: readonly VizRow[] | undefined;
  text: GroupMembersPresenterProps["text"];
}) {
  if (!rows || rows.length === 0) return null;
  return (
    <DataTable
      label="Group members"
      density="compact"
      columns={["Name", "Size"]}
      rows={rows}
      getRowKey={rowKey}
      renderCells={(row) => [
        row.displayName,
        row.quantity ? text.quantity(row.quantity) : text.size(row.size),
      ]}
    />
  );
}

function rowKey(row: VizRow) {
  if (row.itemRef)
    return JSON.stringify([
      "item",
      row.itemRef.resultVersion,
      row.itemRef.itemNo,
    ]);
  if (row.groupRef !== null) return JSON.stringify(["group", row.groupRef]);
  return JSON.stringify(["unidentified", row.displayName, row.size]);
}
