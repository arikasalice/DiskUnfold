import { useEffect, useState } from "react";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { AnalysisClient, DisplayPreferences, SizeValue } from "../../ipc";
import { windowsSizeDisplay } from "./space-format";
import type { SizeDisplay } from "./space-format";

type PreferenceResult = {
  client: AnalysisClient;
  response?: DisplayPreferences;
  unavailable?: boolean;
};

export type AnalysisDisplayPreferences = {
  display: SizeDisplay;
  basis: SizeValue["basis"];
  warning: string | undefined;
};

/** Reads display settings independently of the selected scan result. */
export function useDisplayPreferences(
  client: AnalysisClient | undefined,
): AnalysisDisplayPreferences {
  const [result, setResult] = useState<PreferenceResult>();
  useEffect(() => {
    if (!client) return;
    let active = true;
    const read = () => {
      void client.call("space.display_preferences", {}).then(
        (response) => {
          if (!active) return;
          setResult((current) =>
            current?.client === client &&
            current.response &&
            BigInt(response.settingsRevision) <
              BigInt(current.response.settingsRevision)
              ? current
              : { client, response },
          );
        },
        () => {
          if (!active) return;
          setResult((current) =>
            current?.client === client && current.response
              ? current
              : { client, unavailable: true },
          );
        },
      );
    };
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "settings.changed") read();
    });
    read();
    return () => {
      active = false;
      unsubscribe();
    };
  }, [client]);

  const current = result?.client === client ? result : undefined;
  const response = current?.response;
  if (!response)
    return {
      display: windowsSizeDisplay,
      basis: "allocated",
      warning: current?.unavailable ? "settings.restore_failed" : undefined,
    };
  const { unitSystem, unit } = response;
  const index = SIZE_UNIT_RULES[unitSystem].symbols.findIndex(
    (symbol) => symbol === unit,
  );
  return {
    display: { system: unitSystem, unit: index < 0 ? "auto" : index },
    basis: response.defaultBasis,
    warning: response.warningKey ?? undefined,
  };
}
