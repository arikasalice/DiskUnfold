import type { ReactNode } from "react";
import "../theme/text-document.css";

/**
 * A read-only, keyboard-scrollable block of preformatted text such as a
 * license. The region is focusable so keyboard users can scroll it.
 */
export function TextDocument({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div
      className="du-text-document"
      role="region"
      aria-label={label}
      tabIndex={0}
    >
      <pre>{children}</pre>
    </div>
  );
}
