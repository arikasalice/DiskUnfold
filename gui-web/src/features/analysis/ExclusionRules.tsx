import { Fragment, useState } from "react";
import { Action, NameText } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { ScopeExclusion } from "../../ipc";
import { exclusionText } from "./scope-exclusion";

/** How many rules show before the rest fold behind the toggle. */
const SHOWN_RULES = 3;

/** A path rule breaks after its separators like a scanned path; a pattern reads as written. */
function Rule({ rule }: { rule: ScopeExclusion }) {
  return rule.kind === "path" ? (
    <NameText path name={exclusionText(rule)} />
  ) : (
    exclusionText(rule)
  );
}

/**
 * The value of the summary's "Exclusions" row: the exclusion rules of the
 * scope the shown result was analyzed with. `rules` is undefined when that
 * scope is not known, which reads as unavailable.
 *
 * Up to three rules show. Further rules fold behind a toggle at the end of
 * the line that expands them in place and then folds them again. The toggle
 * stays the same element, so focus stays on it. Another list of rules, such
 * as the one of a new analysis, starts folded.
 */
export function ExclusionRules({
  rules,
}: {
  rules: readonly ScopeExclusion[] | undefined;
}) {
  const text = useLocaleText();
  const [expandedFor, setExpandedFor] = useState<readonly ScopeExclusion[]>();
  if (!rules) return text("ui.analysis.unavailable", "Unavailable");
  const expanded = expandedFor === rules;
  const folded = rules.length - SHOWN_RULES;
  const shown = expanded || folded <= 0 ? rules : rules.slice(0, SHOWN_RULES);
  const count = BigInt(folded);
  return (
    <>
      {shown.map((rule, index) => (
        <Fragment key={index}>
          {index > 0 && ", "}
          <Rule rule={rule} />
        </Fragment>
      ))}
      {folded > 0 && (
        <>
          {" "}
          <Action
            variant="quiet"
            aria-expanded={expanded}
            aria-label={
              expanded
                ? text(
                    "ui.analysis.exclusions_less_label",
                    "Show less of the exclusions",
                  )
                : text(
                    "ui.analysis.exclusions_more_label",
                    folded === 1
                      ? "{count} more exclusion"
                      : "{count} more exclusions",
                    { count },
                  )
            }
            onPress={() => setExpandedFor(expanded ? undefined : rules)}
          >
            {expanded
              ? text("ui.analysis.exclusions_less", "Show less")
              : text("ui.analysis.exclusions_more", "{count} more", { count })}
          </Action>
        </>
      )}
    </>
  );
}
