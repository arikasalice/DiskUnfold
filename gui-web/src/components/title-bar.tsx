import { createContext, useContext, useEffect, useState } from "react";
import type { MouseEvent, ReactNode } from "react";
import { BrandMark } from "./presentation";

type Setter = (label: string | undefined) => void;

const ContextLabel = createContext<string | undefined>(undefined);
const ContextSetter = createContext<Setter>(() => undefined);

/**
 * Holds the label that the title bar shows next to the app name.
 *
 * The document title carries the same words ("DiskUnfold — Screen"), so the system window
 * caption (Windows) and header bar (Linux) name the screen the same way the page bar does.
 */
export function TitleBarProvider({ children }: { children: ReactNode }) {
  const [label, setLabel] = useState<string>();
  useEffect(() => {
    document.title = label ? `DiskUnfold — ${label}` : "DiskUnfold";
  }, [label]);
  return (
    <ContextSetter.Provider value={setLabel}>
      <ContextLabel.Provider value={label}>{children}</ContextLabel.Provider>
    </ContextSetter.Provider>
  );
}

/** Names the screen that is shown, for the title bar. The label clears when the screen leaves. */
export function useTitleBarContext(label: string | undefined) {
  const set = useContext(ContextSetter);
  useEffect(() => {
    set(label);
    return () => set(undefined);
  }, [label, set]);
}

const INTERACTIVE =
  "a, button, input, select, textarea, summary, [role='button'], [tabindex]:not([tabindex='-1']), [data-du-no-drag]";

/**
 * Window bar that the page draws when the native title bar is transparent (macOS).
 *
 * The native close, minimize and zoom buttons stay in place and keep their own keyboard and
 * screen reader access. The bar starts after them: the host publishes the width they occupy
 * as `--du-titlebar-inset-start`. Pressing the empty part of the bar moves the window,
 * a double click zooms it, and any control placed inside the bar keeps its own behavior.
 * Nothing renders where the native frame stays.
 */
export function TitleBar({
  icon,
  enabled,
  onDragStart,
  onZoom,
}: {
  icon: string;
  /** True when the host draws no native title bar; otherwise nothing renders. */
  enabled: boolean;
  /** Asks the host to move the window with the mouse press being handled. */
  onDragStart: () => void;
  /** Asks the host to zoom the window. */
  onZoom: () => void;
}) {
  const label = useContext(ContextLabel);
  if (!enabled) return null;
  const onMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    if ((event.target as Element).closest(INTERACTIVE)) return;
    if (event.detail > 1) return;
    onDragStart();
  };
  const onDoubleClick = (event: MouseEvent<HTMLDivElement>) => {
    if ((event.target as Element).closest(INTERACTIVE)) return;
    onZoom();
  };
  return (
    <div
      className="du-titlebar"
      data-du-titlebar=""
      // The native window title keeps the accessible name of the window; the bar only repeats it.
      aria-hidden="true"
      onMouseDown={onMouseDown}
      onDoubleClick={onDoubleClick}
    >
      <BrandMark src={icon} />
      <span className="du-titlebar__name">DiskUnfold</span>
      {label && <span className="du-titlebar__context">{label}</span>}
    </div>
  );
}
