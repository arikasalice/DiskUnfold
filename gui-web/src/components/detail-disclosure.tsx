import { useId, useState } from "react";
import type { ReactNode } from "react";
import { Action } from "./controls";
import "../theme/detail-disclosure.css";

/** A compact, keyboard-accessible section for optional supporting details. */
export function DetailDisclosure({
  title,
  children,
  defaultExpanded = false,
}: {
  title: string;
  children: ReactNode;
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const contentId = useId();
  return (
    <section className="du-detail-disclosure">
      <Action
        variant="quiet"
        aria-expanded={expanded}
        aria-controls={expanded ? contentId : undefined}
        onPress={() => setExpanded((value) => !value)}
      >
        <span aria-hidden="true">{expanded ? "−" : "+"}</span> {title}
      </Action>
      {expanded && <div id={contentId} className="du-detail-disclosure__content">{children}</div>}
    </section>
  );
}
