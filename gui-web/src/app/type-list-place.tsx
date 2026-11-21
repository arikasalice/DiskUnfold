import { useEffect, useMemo } from "react";
import type { RefObject } from "react";
import type { AnalysisClient, ItemRef } from "../ipc";
import type { TypeNarrowing } from "../features/visualization/ModelLegend";
import { TypeList } from "../features/visualization/TypeList";
import type { VizText } from "../viz";
import { createTypeListPort } from "./type-list-port";
import type { TypeListPort } from "./type-list-port";

/**
 * One query.run port for the open space view, closed with it, and whether
 * "Show in list" has the file list open.
 */
export function useTypeListView(
  open: boolean,
  client: AnalysisClient | undefined,
  narrowing: TypeNarrowing | undefined,
) {
  const port = useTypeListPort(open ? client : undefined);
  return { port, listing: !!narrowing?.listing };
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
  text,
  title,
}: {
  port: TypeListPort | undefined;
  resultVersion: string;
  /** The folder the chart shows; its legend counts the files below it. */
  scope: ItemRef;
  narrowing: TypeNarrowing | undefined;
  text: VizText;
  title: RefObject<HTMLHeadingElement | null>;
}) {
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
