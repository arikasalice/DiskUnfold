import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { Icon } from "./presentation";
import "../theme/band.css";

type BandTone = "info" | "warning" | "error" | "success";
type BandActions = readonly [ReactNode] | readonly [ReactNode, ReactNode];

/** True when the focus sits nowhere useful: on the page body or inside a part that was hidden. */
function focusIsAdrift() {
  const active = document.activeElement;
  return (
    active === null || active === document.body || active.closest("[hidden]")
  );
}

/**
 * A strip of words with at most two actions, pinned to the top of the main
 * area so it stays visible while the page scrolls. The tone is carried by an
 * icon and the words, never by colour alone.
 *
 * The strip does not announce itself; a caller that needs the change spoken
 * uses a live region. When `focusable` is set, the words are the first stop
 * of the strip's focus order, followed by the actions, and they take the
 * focus when the strip appears while the focus is adrift (for example inside
 * a part that was just hidden).
 */
export function Band({
  label,
  tone = "info",
  actions,
  focusable = false,
  children,
}: {
  /** Accessible name of the strip. */
  label: string;
  tone?: BandTone;
  /** At most two actions, in focus order: the main one first. */
  actions?: BandActions;
  focusable?: boolean;
  children: ReactNode;
}) {
  const words = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    if (focusable && focusIsAdrift()) words.current?.focus();
  }, [focusable]);
  return (
    <section className={`du-band du-band--${tone}`} aria-label={label}>
      <Icon name={tone} />
      <p
        ref={words}
        className="du-band__words"
        tabIndex={focusable ? 0 : undefined}
      >
        {children}
      </p>
      {actions && <div className="du-band__actions">{actions}</div>}
    </section>
  );
}
