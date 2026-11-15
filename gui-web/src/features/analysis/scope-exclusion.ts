import type { ScopeExclusion } from "../../ipc";

/**
 * The text of one exclusion rule: the path of a path rule, the pattern of a
 * name pattern rule as written. Every view that lists the rules of a scope
 * shows them with this text.
 */
export function exclusionText(rule: ScopeExclusion): string {
  return rule.kind === "path" ? rule.path.display : rule.pattern;
}
