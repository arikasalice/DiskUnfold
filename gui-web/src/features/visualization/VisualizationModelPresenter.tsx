import { useMemo } from "react";
import {
  Action,
  DataTable,
  DetailDisclosure,
  GeometryRenderer,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import type { VizModel, VizRenderRequest, VizRowsRequest } from "../../ipc";
import { adaptVizModel, selectionKey } from "./model-adapter";
import type {
  ModelResponse,
  ModelRowsPage,
  VizSelection,
} from "./model-adapter";
import { rawSourceSize, shapeLabels } from "./model-text";
import type { VizText } from "./model-text";

type ModelViewProps = {
  frame: ModelResponse;
  page?: ModelRowsPage;
  /** Supplied only while this exact row request is in flight. */
  pendingRowsRequest?: VizRowsRequest;
  view: Pick<VizRenderRequest, "viewport" | "visibleWindow">;
  selected: VizSelection | null;
  onSelect: (selection: VizSelection) => void;
  onPage?: (request: VizRowsRequest) => void;
  text: VizText;
};

/** A committed model only. Request ordering and last-valid-model retention belong to its controller. */
export function VisualizationModelPresenter(props: ModelViewProps) {
  const { frame, page, view, selected, text } = props;
  const rowsPending =
    props.pendingRowsRequest?.modelRef === frame.model.modelRef &&
    props.pendingRowsRequest.resultRevision === frame.resultRevision;
  const adapted = useMemo(
    () =>
      adaptVizModel(frame, page, (shape, row) =>
        shapeLabels(frame.model, shape, row, text),
      ),
    [frame, page, text],
  );
  const rowsContent =
    adapted.rowsMatch && page ? (
      <ModelRows
        page={page}
        model={frame.model}
        text={text}
        {...(props.onPage ? { onPage: props.onPage } : {})}
      />
    ) : rowsPending ? null : (
      <Notice>Rows have not been retrieved for this graph.</Notice>
    );
  const rows = (
    <>
      {rowsPending && <Progress label="Loading rows from this graph" />}
      {rowsContent}
    </>
  );
  if (!adapted.validTargets)
    return (
      <Notice tone="error">
        The graph contains inconsistent item references. Reload this result.
      </Notice>
    );
  return (
    <section className="du-review-stack" aria-label="Storage visualization">
      <div className="du-review-context">
        <StatusBadge
          tone={frame.completeness === "complete" ? "neutral" : "warning"}
        >
          {text.key(frame.completeness)}
        </StatusBadge>
        <StatusBadge>{text.key(frame.model.origin)}</StatusBadge>
        <StatusBadge
          tone={
            frame.model.staleness.state === "current" ? "neutral" : "warning"
          }
        >
          {text.key(frame.model.staleness.state)}
        </StatusBadge>
        <Text>{text.observedAt(frame.observedAt)}</Text>
      </div>
      {frame.model.shapes.length === 0 ? (
        <>
          <Notice>
            No drawable items in this result. Review its totals and unavailable
            values below.
          </Notice>
          {rows}
        </>
      ) : (
        <GeometryRenderer
          modelRef={frame.model.modelRef}
          resultRevision={frame.resultRevision}
          label="Space distribution"
          selectedText="selected"
          fallbackNotice="This graph cannot be drawn. The rows below belong to the same result."
          missingFallbackNotice="Rows from this result are unavailable."
          width={view.viewport.width}
          height={view.viewport.height}
          viewport={view.visibleWindow}
          shapes={adapted.shapes}
          relations={adapted.relations}
          selectedRowRef={selected ? selectionKey(selected, frame) : null}
          onSelect={(_rowKey, shapeRef) => {
            const selection = adapted.targetsByShape.get(shapeRef);
            if (selection) props.onSelect(selection);
          }}
          fallback={{
            modelRef: frame.model.modelRef,
            resultRevision: frame.resultRevision,
            content: rows,
          }}
        />
      )}
      <ModelEvidence frame={frame} text={text} />
    </section>
  );
}

function ModelRows({
  page,
  model,
  text,
  onPage,
}: {
  page: ModelRowsPage;
  model: VizModel;
  text: VizText;
  onPage?: (request: VizRowsRequest) => void;
}) {
  const { request, response } = page;
  const next = request.start + response.rows.length;
  return (
    <DetailDisclosure
      title="Rows from this graph"
      key={`${request.modelRef}:${request.resultRevision}`}
    >
      <Text>
        {response.rows.length === 0
          ? "No rows in this page"
          : `Rows ${request.start + 1}–${next} of ${response.totalCount}`}
      </Text>
      <DataTable
        label="Loaded graph rows"
        density="compact"
        columns={[
          "Name",
          "Value",
          "Ratio",
          "Denominator",
          "Classification",
          "Flags",
        ]}
        rows={response.rows}
        getRowKey={(row) => JSON.stringify([row.itemRef, row.groupRef])}
        renderCells={(row) => [
          row.displayName,
          row.quantity ? text.quantity(row.quantity) : text.size(row.size),
          row.ratio ?? "Unavailable",
          `${text.quantity(row.denominator.value)} · ${text.key(row.denominator.basis)}`,
          text.key(
            model.legend.find((entry) => entry.key === row.legendClassKey)
              ?.labelKey ?? row.legendClassKey,
          ),
          row.displayFlags.map(text.key).join(" · "),
        ]}
      />
      {onPage && (
        <div className="du-review-actions">
          <Action
            isDisabled={request.start === 0}
            onPress={() =>
              onPage({
                ...request,
                start: Math.max(0, request.start - request.count),
              })
            }
          >
            Previous rows
          </Action>
          <Action
            isDisabled={
              next >= response.totalCount || response.rows.length === 0
            }
            onPress={() => onPage({ ...request, start: next })}
          >
            Next rows
          </Action>
        </div>
      )}
    </DetailDisclosure>
  );
}

function ModelEvidence({
  frame,
  text,
}: {
  frame: ModelResponse;
  text: VizText;
}) {
  const { model } = frame;
  const totals = [
    ["Scope total", model.totals.scope],
    ["Drawn total", model.totals.drawn],
    ["Grouped total", model.totals.grouped],
    ["Filtered out total", model.totals.filteredOut],
  ] as const;
  return (
    <DetailDisclosure title="Measurement basis and totals">
      <Readout
        label="Representation"
        rows={[
          { label: "Format", value: text.key(model.representation.format) },
          { label: "Measure", value: text.key(model.representation.measure) },
          {
            label: "Size basis",
            value: text.key(model.representation.sizeBasis),
          },
          {
            label: "Denominator",
            value: `${text.quantity(model.denominator.value)} · ${text.key(model.denominator.basis)}`,
          },
        ]}
      />
      <Readout
        label="Independent totals"
        rows={totals.map(([label, total]) => ({
          label,
          value: text.quantity(total.quantity),
        }))}
      />
      <Readout
        label="Source sizes"
        rows={totals.map(([label, total]) => ({
          label,
          value:
            total.sourceSize === null
              ? "No source size supplied"
              : rawSourceSize(total.sourceSize),
        }))}
      />
      <Readout
        label="Coverage"
        rows={[
          {
            label: "Grouped items",
            value: text.quantity(model.summary.groupedItems),
          },
          {
            label: "Omitted items",
            value: text.quantity(model.summary.omittedItems),
          },
          {
            label: "Unknown items",
            value: text.quantity(model.summary.unknownItems),
          },
        ]}
      />
      {model.summary.reasonKeys.map((key) => (
        <Notice key={key}>{text.key(key)}</Notice>
      ))}
      {model.staleness.reasonKey && (
        <Notice>{text.key(model.staleness.reasonKey)}</Notice>
      )}
    </DetailDisclosure>
  );
}
