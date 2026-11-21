import { Action } from "./controls";
import { NameText, hasPathSeparator } from "./name-text";
import "../theme/breadcrumbs.css";

export type BreadcrumbEntry = {
  id: string;
  label: string;
  tooltip?: string | undefined;
  accessibleLabel?: string | undefined;
  current?: boolean;
  onPress?: () => void;
};

/**
 * Navigation within a location hierarchy. Every label, on an ancestor action
 * or as text, is drawn as a name, so white space at its edges is marked on
 * screen and an action reads as the real name followed by an explanation of
 * that white space. A label that is not a file or folder name, such as a
 * translated root label, has no edge white space and renders as plain text.
 * The `title` keeps the real label.
 *
 * The hierarchy uses the remaining width and wraps onto more lines; no label
 * is cut. A label that is a path, such as a scope root, breaks after its
 * separators first.
 */
export function Breadcrumbs({
  label,
  entries,
}: {
  label: string;
  entries: readonly BreadcrumbEntry[];
}) {
  return (
    <nav className="du-breadcrumbs" aria-label={label}>
      <ol role="list">
        {entries.map((entry, index) => (
          <li
            key={entry.id}
            title={entry.tooltip ?? entry.label}
            aria-label={entry.accessibleLabel}
          >
            {index > 0 && <span aria-hidden="true">/</span>}
            {entry.onPress && !entry.current ? (
              <Action
                onPress={entry.onPress}
                {...(entry.accessibleLabel
                  ? { "aria-label": entry.accessibleLabel }
                  : {})}
              >
                <NameText
                  name={entry.label}
                  path={hasPathSeparator(entry.label)}
                />
              </Action>
            ) : (
              <span
                aria-current={entry.current ? "location" : undefined}
                title={entry.tooltip ?? entry.label}
              >
                <NameText
                  name={entry.label}
                  path={hasPathSeparator(entry.label)}
                />
              </span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}
