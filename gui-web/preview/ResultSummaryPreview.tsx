import { useState } from "react";
import { SelectField } from "../src/components";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import { LocaleTextContext } from "../src/i18n/presentation";
import "../src/theme/analysis-workspace.css";
import "../src/theme/analysis-results.css";
import {
  exclusionStateLabels,
  exclusionStates,
  koreanText,
  summaryClient,
  summaryRoots,
  summaryTotal,
} from "./result-summary-fixture";
import type { ExclusionState } from "./result-summary-fixture";

const preferences = {
  display: { system: "iec", unit: "auto" },
  basis: "logical",
  warning: undefined,
} as const;

/** The state named by ?exclusions= in the address, for a capture that opens on it. */
function requestedState(): ExclusionState {
  const requested = new URLSearchParams(window.location.search).get(
    "exclusions",
  );
  return requested !== null && Object.hasOwn(exclusionStates, requested)
    ? (requested as ExclusionState)
    : "two";
}

/**
 * The product's result summary on a fixed result, inside the frame the
 * analysis screen gives it, in Korean. The select changes only the exclusion
 * rules of the scope the result was analyzed with.
 */
export function ResultSummaryPreview() {
  const [state, setState] = useState(requestedState);
  const summary = useSummaryBatch(summaryClient, summaryTotal, false, false);
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <SelectField
        label="제외 규칙"
        value={state}
        options={(Object.keys(exclusionStates) as ExclusionState[]).map(
          (value) => ({ value, label: exclusionStateLabels[value] }),
        )}
        onChange={setState}
      />
      <div className="du-analysis du-analysis--workspace">
        <div className="du-analysis__grid" data-result-active>
          <section
            className="du-analysis__result"
            aria-label={koreanText("ui.analysis.scan_and_results", "")}
          >
            <section
              className="du-result__body"
              aria-label={koreanText("ui.analysis.result_details", "")}
              tabIndex={0}
            >
              <PinnedResult
                client={summaryClient}
                total={summaryTotal}
                latestVersion={summaryTotal.resultVersion}
                volume={summary.volume}
                scan={undefined}
                wait={undefined}
                roots={summaryRoots}
                exclusions={exclusionStates[state]}
                preferences={preferences}
                displayChoice={undefined}
                onDisplayChoice={() => {}}
                onReopenLatest={() => {}}
              />
            </section>
          </section>
        </div>
      </div>
    </LocaleTextContext.Provider>
  );
}
