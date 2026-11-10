import { useLayoutEffect, useRef, useState } from "react";
import type {
  ComponentProps,
  KeyboardEvent,
  ReactNode,
  RefObject,
} from "react";
import {
  Cell,
  CollectionRendererContext,
  Column,
  DefaultCollectionRenderer,
  Row,
  Table,
  TableBody,
  TableHeader,
} from "react-aria-components";
import { canHandleBrowseKey } from "./browse-keys";
import { useBrowseGridViewport } from "./browse-grid-viewport";
import "../theme/browse-grid.css";

const virtualRows = { ...DefaultCollectionRenderer, isVirtualized: true };

export type BrowseGridFocus = {
  index: number;
  eligible: () => boolean;
  finish: () => void;
};

type GridProps<Item> = {
  label: string;
  columns: readonly string[];
  count: number;
  rowCount: number;
  busy: boolean;
  focusRequest: BrowseGridFocus | undefined;
  onMove: (index: number, end: boolean) => void;
  onOpen: (item: Item) => void;
  onFocusRow: (index: number | undefined) => void;
  onVisible: (start: number, end: number) => void;
  onLayoutScroll?: (before: number, after: number) => void;
  getRow: (index: number) => Item | undefined;
  getKey: (item: Item) => string;
  renderCells: (item: Item) => readonly ReactNode[];
};

function rowIndex(target: Element | null): number | undefined {
  const value = target?.closest<HTMLElement>("[data-row]")?.dataset.row;
  return value === undefined ? undefined : Number(value);
}

function GridSurface({
  element,
  body,
  request,
  reveal,
}: {
  element: ComponentProps<"div">;
  body: RefObject<HTMLDivElement | null>;
  request: BrowseGridFocus | undefined;
  reveal: (index: number) => boolean;
}) {
  const handled = useRef<BrowseGridFocus>(undefined);
  const [revealed, setRevealed] = useState<BrowseGridFocus>();
  useLayoutEffect(() => {
    if (!request || handled.current === request) return;
    const target = body.current?.querySelector<HTMLElement>(
      `[data-row="${request.index}"]`,
    );
    if (!target) return;
    if (request.eligible() && revealed !== request && reveal(request.index)) {
      setRevealed(request);
      return;
    }
    handled.current = request;
    if (request.eligible()) {
      target.focus({ preventScroll: true });
    }
    request.finish();
  });
  return <div {...element} />;
}

/** A sparse, bounded React Aria collection with absolute row positions. */
export function BrowseGrid<Item>({
  label,
  columns,
  count,
  rowCount,
  busy,
  focusRequest,
  onMove,
  onOpen,
  onFocusRow,
  onVisible,
  onLayoutScroll,
  getRow,
  getKey,
  renderCells,
}: GridProps<Item>) {
  const viewport = useBrowseGridViewport(count, onLayoutScroll);
  const [focused, setFocused] = useState<number>();
  const pins = [focused, focusRequest?.index].filter(
    (index): index is number => index !== undefined,
  );
  const { visibleStart, visibleEnd, indexes } = viewport.window(pins);
  useLayoutEffect(() => {
    onVisible(visibleStart, visibleEnd);
  }, [onVisible, visibleStart, visibleEnd]);

  function trackFocus(index: number | undefined) {
    setFocused(index);
    onFocusRow(index);
  }

  function handleKey(event: KeyboardEvent<HTMLDivElement>) {
    if (!canHandleBrowseKey(event) || event.altKey) return;
    const index = rowIndex(
      event.target instanceof Element ? event.target : null,
    );
    const moves = {
      ArrowUp: Math.max(0, (index ?? 1) - 1),
      ArrowDown: Math.min(count - 1, (index ?? -1) + 1),
      Home: 0,
      End: count - 1,
    };
    const target = Object.hasOwn(moves, event.key)
      ? moves[event.key as keyof typeof moves]
      : undefined;
    if (target !== undefined) {
      event.preventDefault();
      event.stopPropagation();
      onMove(target, event.key === "End");
    } else if (event.key === "Enter" && index !== undefined) {
      event.preventDefault();
      event.stopPropagation();
      const item = getRow(index);
      if (!event.repeat && item !== undefined) onOpen(item);
    }
  }

  return (
    <div
      className="du-browse-grid-frame"
      onKeyDownCapture={handleKey}
      onFocusCapture={(event) => trackFocus(rowIndex(event.target))}
      onBlurCapture={(event) => {
        if (
          !(event.relatedTarget instanceof Node) ||
          !event.currentTarget.contains(event.relatedTarget)
        ) {
          trackFocus(undefined);
        }
      }}
    >
      <CollectionRendererContext.Provider value={virtualRows}>
        <Table
          aria-label={label}
          className="du-browse-grid"
          selectionMode="multiple"
          selectionBehavior="toggle"
          render={(props) => (
            <GridSurface
              element={{
                ...(props as ComponentProps<"div">),
                "aria-rowcount": rowCount,
                "aria-busy": busy || undefined,
              }}
              body={viewport.body}
              request={focusRequest}
              reveal={viewport.reveal}
            />
          )}
        >
          <TableHeader className="du-browse-grid__header">
            {columns.map((column, index) => (
              <Column id={column} key={column} isRowHeader={index === 0}>
                {column}
              </Column>
            ))}
          </TableHeader>
          <TableBody
            ref={viewport.attachBody}
            className="du-browse-grid__body"
            style={{ blockSize: viewport.height }}
          >
            {indexes.flatMap((index) => {
              const item = getRow(index);
              if (item === undefined) return [];
              const key = getKey(item);
              return [
                <Row
                  id={key}
                  key={key}
                  ref={viewport.watchRow}
                  className="du-browse-grid__row"
                  style={{ insetBlockStart: viewport.rowTop(index) }}
                  render={(props) => (
                    <div
                      {...(props as ComponentProps<"div">)}
                      aria-rowindex={index + 2}
                      data-row={index}
                    />
                  )}
                >
                  {renderCells(item).map((cell, column) => (
                    <Cell key={columns[column]} focusMode="cell">
                      <span
                        className="du-browse-grid__label"
                        aria-hidden="true"
                      >
                        {columns[column]}
                      </span>
                      {cell}
                    </Cell>
                  ))}
                </Row>,
              ];
            })}
          </TableBody>
        </Table>
      </CollectionRendererContext.Provider>
    </div>
  );
}
