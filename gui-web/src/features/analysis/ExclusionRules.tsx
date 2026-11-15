import { useState } from "react";
import { Action, NameText, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { ScopeExclusion } from "../../ipc";
import "./exclusion-rules.css";

/**
 * One rule. A path breaks after its separators like a scanned path. A name
 * pattern reads as written behind a muted "Name pattern", so a pattern such
 * as "node_modules" is not taken for a path.
 */
function Rule({ rule }: { rule: ScopeExclusion }) {
  const text = useLocaleText();
  if (rule.kind === "name_pattern")
    return (
      <>
        <div className="du-exclusion-rules__kind">
          {text("ui.analysis.exclusion_name_pattern", "Name pattern")}
        </div>{" "}
        {rule.pattern}
      </>
    );
  return (
    <>
      <NameText
        path
        name={
          rule.path.display ||
          text("ui.analysis.recent_empty_path", "(empty path)")
        }
      />
      {rule.path.lossy && (
        <Text kind="muted">
          {text(
            "ui.analysis.recent_path_characters_hidden",
            "Some characters cannot be displayed.",
          )}
        </Text>
      )}
    </>
  );
}

/**
 * The exclusion rules of a scope as a list, one rule on each line. A path
 * can hold commas and spaces, so rules are never joined on one line. Every
 * view that lists the rules of a scope draws them with this.
 *
 * `rules` is undefined when the scope is not known, which reads as
 * unavailable. With `foldAfter`, only that many rules show at first. The
 * rest fold behind a toggle on the line below the list, which expands them
 * in place and then folds them again. The toggle stays the same element, so
 * focus stays on it. Another list of rules, such as the one of a new
 * analysis, starts folded.
 */
export function ExclusionRules({
  rules,
  foldAfter,
}: {
  rules: readonly ScopeExclusion[] | undefined;
  /** How many rules show before the rest fold; without it all rules show. */
  foldAfter?: number;
}) {
  const text = useLocaleText();
  const [expandedFor, setExpandedFor] = useState<readonly ScopeExclusion[]>();
  if (!rules) return text("ui.analysis.unavailable", "Unavailable");
  const expanded = expandedFor === rules;
  const folded = foldAfter === undefined ? 0 : rules.length - foldAfter;
  const shown = expanded || folded <= 0 ? rules : rules.slice(0, foldAfter);
  const count = BigInt(folded);
  return (
    <div className="du-exclusion-rules">
      <div className="du-exclusion-rules__list" role="list">
        {shown.map((rule, index) => (
          <div key={index} role="listitem">
            <Rule rule={rule} />
          </div>
        ))}
      </div>
      {folded > 0 && (
        <Action
          variant="quiet"
          aria-expanded={expanded}
          aria-label={
            expanded
              ? text(
                  "ui.analysis.exclusions_less_label",
                  "Show fewer exclusions",
                )
              : text(
                  "ui.analysis.exclusions_more_label",
                  folded === 1
                    ? "Show {count} more exclusion"
                    : "Show {count} more exclusions",
                  { count },
                )
          }
          onPress={() => setExpandedFor(expanded ? undefined : rules)}
        >
          {expanded
            ? text("ui.analysis.exclusions_less", "Show less")
            : text("ui.analysis.exclusions_more", "{count} more", { count })}
        </Action>
      )}
    </div>
  );
}
