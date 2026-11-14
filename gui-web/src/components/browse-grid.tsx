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
import {
  BROWSE_SCREEN,
  browseKeyTable,
  canHandleBrowseKey,
} from "./browse-keys";
import { useBrowseGridViewport } from "./browse-grid-viewport";
import "../theme/browse-grid.css";

const virtualRows = { ...DefaultCollectionRenderer, isVirtualized: true };

function isGridKey(key: string) {
  return browseKeyTable.usesKey(BROWSE_SCREEN, key);
}

/** The row a navigation action moves to, or undefined for other actions. */
function moveTarget(
  action: string | undefined,
  index: number | undefined,
  count: number,
  pageRows: number,
): number | undefined {
  switch (action) {
    case "move_previous":
      return Math.max(0, (index ?? 1) - 1);
    case "move_next":
      return Math.min(count - 1, (index ?? -1) + 1);
    case "move_first":
      return 0;
    case "move_last":
      return count - 1;
    case "move_page_up":
      return Math.max(0, (index ?? 0) - pageRows);
    case "move_page_down":
      return Math.min(count - 1, (index ?? 0) + pageRows);
    default:
      return undefined;
  }
}

export type BrowseGridFocus = {
  index: number;
  eligible: () => boolean;
  finish: () => void;
};

type GridProps<Item> = {
  scrollRoot?: RefObject<HTMLDivElement | null>;
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

function guardedKeys(handler: ComponentProps<"div">["onKeyDown"]) {
  return (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target instanceof Element && event.target.closest("button"))
      return;
    if (!isGridKey(event.key) || (canHandleBrowseKey(event) && !event.altKey))
      handler?.(event);
  };
}

function GridSurface({
  element,
  body,
  request,
  reveal,
  rowTop,
}: {
  element: ComponentProps<"div">;
  body: RefObject<HTMLDivElement | null>;
  request: BrowseGridFocus | undefined;
  reveal: (index: number) => boolean;
  rowTop: (index: number) => number;
}) {
  const handled = useRef<BrowseGridFocus>(undefined);
  const [revealed, setRevealed] = useState<BrowseGridFocus>();
  useLayoutEffect(() => {
    if (!request || handled.current === request) return;
    const target = body.current?.querySelector<HTMLElement>(
      `[data-row="${request.index}"]`,
    );
    if (!target) return;
    const eligible = request.eligible();
    if (eligible && revealed !== request && reveal(request.index)) {
      setRevealed(request);
      return;
    }
    // React Aria's collection can commit its new row styles after the
    // document has scrolled. Keep the origin until the row is positioned.
    if (
      eligible &&
      revealed === request &&
      Math.abs(
        Number.parseFloat(target.style.insetBlockStart) - rowTop(request.index),
      ) >= 0.5
    )
      return;
    handled.current = request;
    if (eligible && request.eligible()) {
      target.focus({ preventScroll: true });
    }
    request.finish();
  });
  return <div {...element} />;
}

/** A sparse, bounded React Aria collection with absolute row positions. */
export function BrowseGrid<Item>({
  scrollRoot,
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
  const ownRoot = useRef<HTMLDivElement>(null);
  const root = scrollRoot ?? ownRoot;
  const viewport = useBrowseGridViewport(count, root, onLayoutScroll);
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

  function handleKey(
    event: KeyboardEvent<HTMLDivElement>,
    toggle: (key: string) => void,
  ) {
    if (
      !isGridKey(event.key) ||
      !(event.target instanceof Element) ||
      !event.currentTarget.contains(event.target) ||
      event.target.closest("button")
    )
      return;
    const index = rowIndex(event.target);
    if (!canHandleBrowseKey(event) || event.altKey) return;
    const action = browseKeyTable.find(BROWSE_SCREEN, event)?.action;
    const target = moveTarget(action, index, count, viewport.pageRows);
    if (target !== undefined) {
      event.preventDefault();
      event.stopPropagation();
      onMove(target, action === "move_last");
    } else if (
      (action === "open_item" || action === "toggle_selection") &&
      index !== undefined
    ) {
      event.preventDefault();
      event.stopPropagation();
      const item = getRow(index);
      if (event.repeat || item === undefined) return;
      if (action === "toggle_selection") toggle(getKey(item));
      else onOpen(item);
    }
  }

  return (
    <div
      ref={root}
      tabIndex={-1}
      className="du-browse-grid-frame"
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
          render={(props, { state }) => {
            const element = props as ComponentProps<"div">;
            return (
              <GridSurface
                element={{
                  ...element,
                  "aria-rowcount": rowCount,
                  "aria-busy": busy || undefined,
                  onKeyDown: guardedKeys(element.onKeyDown),
                  onKeyDownCapture: (event) => {
                    element.onKeyDownCapture?.(event);
                    handleKey(event, (key) =>
                      state.selectionManager.toggleSelection(key),
                    );
                  },
                }}
                body={viewport.body}
                request={focusRequest}
                reveal={viewport.reveal}
                rowTop={viewport.rowTop}
              />
            );
          }}
        >
          <TableHeader className="du-browse-grid__header">
            {columns.map((column, index) => (
              <Column
                id={column}
                key={column}
                isRowHeader={index === 0}
                render={(props) => {
                  const element = props as ComponentProps<"div">;
                  return (
                    <div
                      {...element}
                      onKeyDown={guardedKeys(element.onKeyDown)}
                    />
                  );
                }}
              >
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
                  render={(props) => {
                    const element = props as ComponentProps<"div">;
                    return (
                      <div
                        {...element}
                        onKeyDown={guardedKeys(element.onKeyDown)}
                        aria-rowindex={index + 2}
                        data-row={index}
                        data-item-key={key}
                      />
                    );
                  }}
                >
                  {renderCells(item).map((cell, column) => (
                    <Cell
                      key={columns[column]}
                      focusMode="cell"
                      render={(props) => {
                        const element = props as ComponentProps<"div">;
                        return (
                          <div
                            {...element}
                            onKeyDown={guardedKeys(element.onKeyDown)}
                          />
                        );
                      }}
                    >
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
