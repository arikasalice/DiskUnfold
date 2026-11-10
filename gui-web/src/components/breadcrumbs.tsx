import { Action } from "./controls";
import "../theme/breadcrumbs.css";

export type BreadcrumbEntry = {
  id: string;
  label: string;
  current?: boolean;
  onPress?: () => void;
};

/** Navigation within a location hierarchy. Current locations are text. */
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
          <li key={entry.id}>
            {index > 0 && <span aria-hidden="true">/</span>}
            {entry.onPress && !entry.current ? (
              <Action onPress={entry.onPress}>{entry.label}</Action>
            ) : (
              <span aria-current={entry.current ? "location" : undefined}>
                {entry.label}
              </span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}
