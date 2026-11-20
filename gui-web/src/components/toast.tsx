import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Icon } from "./presentation";
import type { IconName } from "./presentation";
import "../theme/toast.css";

/** How long a short notice stays (F06 README 35). */
export const TOAST_DURATION = 5000;
const FADE = 150;
/** Its distance from the bottom of the window, as in toast.css. */
const BOTTOM = 24;

/**
 * A short notice fixed at the bottom centre of the window: text and an icon,
 * no buttons, never focused. It shows each time `version` changes and leaves
 * after five seconds or at the next key. The same notice again only restarts
 * its time. It moves to the top when it would cover the focused element. It is
 * hidden from assistive technology: the caller says the same words once in its
 * own status region (F06 README 35).
 *
 * The words and `icon` are kept from the moment it shows, so a caller may drop
 * its message before the notice has gone. `icon` tells an outcome by shape.
 */
export function Toast({
  message,
  version,
  icon = "info",
}: {
  message: ReactNode;
  version: number;
  icon?: IconName;
}) {
  const [phase, setPhase] = useState<"hidden" | "shown" | "leaving">("hidden");
  const [shown, setShown] = useState<{ message: ReactNode; icon: IconName }>({
    message,
    icon,
  });
  const [atTop, setAtTop] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const seen = useRef(version);
  const leave = () => {
    clearTimeout(timer.current);
    setPhase((current) => (current === "hidden" ? current : "leaving"));
    timer.current = setTimeout(() => setPhase("hidden"), FADE);
  };
  useEffect(() => {
    if (version === seen.current || !message) return;
    seen.current = version;
    clearTimeout(timer.current);
    setShown({ message, icon });
    setPhase("shown");
    timer.current = setTimeout(leave, TOAST_DURATION);
  }, [version, message, icon]);
  useEffect(() => {
    if (phase !== "shown") return;
    // The next key hides it; a key that brings the same notice again keeps it
    // (the new version arrives in the same turn and wins).
    let pending: ReturnType<typeof setTimeout> | undefined;
    const atVersion = version;
    const onKey = () => {
      pending = setTimeout(() => {
        if (seen.current === atVersion) leave();
      });
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      clearTimeout(pending);
    };
  }, [phase, version]);
  useEffect(() => () => clearTimeout(timer.current), []);
  useLayoutEffect(() => {
    if (phase !== "shown" || !box.current) return;
    // Judged where it stands at the bottom, wherever it stands now.
    const focused = document.activeElement;
    const own = box.current.getBoundingClientRect();
    const bottom = window.innerHeight - BOTTOM;
    const target = focused?.getBoundingClientRect();
    setAtTop(
      !!target &&
        focused !== document.body &&
        target.bottom > bottom - own.height &&
        target.top < bottom &&
        target.right > own.left &&
        target.left < own.right,
    );
  }, [phase, version]);
  if (phase === "hidden") return null;
  return (
    <div
      ref={box}
      className="du-toast"
      aria-hidden="true"
      data-place={atTop ? "top" : "bottom"}
      data-icon={shown.icon}
      data-leaving={phase === "leaving" ? "true" : undefined}
    >
      <Icon name={shown.icon} />
      <span>{shown.message}</span>
    </div>
  );
}
