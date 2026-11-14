import { useLayoutEffect, useRef } from "react";
import { Action } from "./controls";
import { NameText } from "./name-text";
import "../theme/breadcrumbs.css";

export type BreadcrumbEntry = {
  id: string;
  label: string;
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
 */
export function Breadcrumbs({
  label,
  entries,
}: {
  label: string;
  entries: readonly BreadcrumbEntry[];
}) {
  const viewport = useRef<HTMLElement>(null);
  const pathKey = JSON.stringify(
    entries.map(({ id, label: name, current }) => [id, name, current]),
  );
  useLayoutEffect(() => {
    const element = viewport.current;
    if (element) element.scrollLeft = element.scrollWidth;
  }, [pathKey]);
  return (
    <nav ref={viewport} className="du-breadcrumbs" aria-label={label}>
      <ol role="list">
        {entries.map((entry, index) => (
          <li key={entry.id} title={entry.label}>
            {index > 0 && <span aria-hidden="true">/</span>}
            {entry.onPress && !entry.current ? (
              <Action onPress={entry.onPress}>
                <NameText name={entry.label} />
              </Action>
            ) : (
              <span
                aria-current={entry.current ? "location" : undefined}
                title={entry.label}
              >
                <NameText name={entry.label} />
              </span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}
