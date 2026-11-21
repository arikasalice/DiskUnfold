import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import type { AnalysisClient, ItemRef } from "../ipc";
import type { TypeNarrowing } from "../features/visualization/ModelLegend";
import { TypeList } from "../features/visualization/TypeList";
import type { IntervalListing } from "../features/visualization/interval-list";
import type { VizText } from "../viz";
import { createTypeListPort } from "./type-list-port";
import type { TypeListPort } from "./type-list-port";

/**
 * One query.run port for the open space view, closed with it, and whether
 * "Show in list" or a histogram interval (README 52) has the file list open.
 * An interval's list closes when `view` (folder, result and format) changes, and
 * going back returns the focus to the bar or button that opened it.
 */
export function useTypeListView(
  open: boolean,
  client: AnalysisClient | undefined,
  narrowing: TypeNarrowing | undefined,
  view: string,
) {
  const port = useTypeListPort(open ? client : undefined);
  const [interval, setListing] = useState<IntervalListing | null>(null);
  const returning = useRef<IntervalListing["origin"]>(null);
  useEffect(() => setListing(null), [view]);
  useLayoutEffect(() => {
    if (interval || !returning.current) return;
    if (returning.current.isConnected)
      returning.current.focus({ preventScroll: false });
    returning.current = null;
  }, [interval]);
  return {
    port,
    listing: !!narrowing?.listing || !!interval,
    interval,
    openInterval: setListing,
    closeInterval: () => {
      returning.current = interval?.origin ?? null;
      setListing(null);
    },
  };
}

function useTypeListPort(
  client: AnalysisClient | undefined,
): TypeListPort | undefined {
  const port = useMemo(
    () => (client ? createTypeListPort(client) : undefined),
    [client],
  );
  useEffect(() => () => port?.dispose(), [port]);
  return port;
}

/**
 * The file list in the chart's place while "Show in list" is open
 * (F06 README 46.13). The chip's × clears the type and sends the focus to the
 * screen title; going back returns it to "Show in list".
 */
export function TypeListPlace({
  port,
  resultVersion,
  scope,
  narrowing,
  interval,
  onCloseInterval,
  text,
  title,
}: {
  port: TypeListPort | undefined;
  resultVersion: string;
  /** The folder the chart shows; its legend counts the files below it. */
  scope: ItemRef;
  narrowing: TypeNarrowing | undefined;
  /** A histogram interval's list, shown before a type's (README 52). */
  interval: IntervalListing | null;
  onCloseInterval: () => void;
  text: VizText;
  title: RefObject<HTMLHeadingElement | null>;
}) {
  if (port && interval)
    return (
      <TypeList
        source={port}
        resultVersion={resultVersion}
        scope={scope}
        condition={interval.condition}
        type={interval.label}
        interval={interval.label}
        summary={interval.summary}
        text={text}
        onBack={onCloseInterval}
      />
    );
  if (!port || !narrowing?.listing || !narrowing.condition || !narrowing.label)
    return null;
  return (
    <TypeList
      source={port}
      resultVersion={resultVersion}
      scope={scope}
      condition={narrowing.condition}
      type={narrowing.label}
      summary={narrowing.listing}
      text={text}
      onBack={narrowing.closeList}
      onClear={() => {
        narrowing.choose(null);
        title.current?.focus({ preventScroll: true });
      }}
    />
  );
}
