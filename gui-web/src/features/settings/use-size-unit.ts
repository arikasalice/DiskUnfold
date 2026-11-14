import { useEffect, useState } from "react";
import type { AnalysisClient } from "../../ipc";

/** The saved size unit, read from the display preferences that F14 owns. */
export type SizeUnitReadout = { system: "si" | "iec"; unit: string };

type Read = { client: AnalysisClient; value: SizeUnitReadout | undefined };

function readout(body: unknown): SizeUnitReadout | undefined {
  if (typeof body !== "object" || body === null) return undefined;
  const { unitSystem, unit } = body as Record<string, unknown>;
  if ((unitSystem !== "si" && unitSystem !== "iec") || typeof unit !== "string")
    return undefined;
  return { system: unitSystem, unit };
}

/**
 * Reads the saved size unit through `space.display_preferences` and follows
 * `settings.changed`. This only displays the value; it never writes it.
 *
 * Passing `undefined` (analysis is off) makes no request. A failed or
 * malformed read yields `undefined`, and the caller hides the row.
 */
export function useSizeUnit(
  client: AnalysisClient | undefined,
): SizeUnitReadout | undefined {
  const [read, setRead] = useState<Read>();
  useEffect(() => {
    if (!client) return;
    let active = true;
    let latest = 0;
    const load = () => {
      const ticket = ++latest;
      void client.call("space.display_preferences", {}).then(
        (body) => {
          if (active && ticket === latest)
            setRead({ client, value: readout(body) });
        },
        () => {
          if (active && ticket === latest)
            setRead({ client, value: undefined });
        },
      );
    };
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "settings.changed") load();
    });
    load();
    return () => {
      active = false;
      unsubscribe();
    };
  }, [client]);
  return read && read.client === client ? read.value : undefined;
}
