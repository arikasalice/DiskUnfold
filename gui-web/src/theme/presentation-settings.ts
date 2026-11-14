/**
 * The one place that decides motion level and screen scale.
 *
 * The decision becomes two root settings that `locale.css` and the size
 * tokens read: `data-du-motion` and `--du-app-scale`. No component decides
 * either on its own.
 */

/** App motion setting (`a11y.motion`). */
export type MotionSetting = "follow_os" | "reduce";

/** Motion level for transitions and expanding effects. */
export type MotionLevel = "standard" | "reduce";

/** Selectable app scale percentages (`a11y.app_scale`), 100 by default. */
export const APP_SCALES = [100, 110, 125, 150, 175, 200] as const;

/** The WebView media query that reports the OS "reduce motion" state. */
export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/** The settings that presentation depends on. */
export type PresentationSettings = Readonly<{
  motion: MotionSetting;
  appScale: number;
}>;

/**
 * Combines the OS state with the app setting. The app can only reduce motion
 * further: OS "reduce" or app "reduce" gives `reduce`. An OS state that could
 * not be read counts as off.
 */
export function resolveMotionLevel(
  setting: MotionSetting,
  osReduces: boolean | undefined,
): MotionLevel {
  return setting === "reduce" || osReduces === true ? "reduce" : "standard";
}

/** Scale as a multiplier. A value outside the selectable set means 100%. */
export function scaleFactor(percent: number): number {
  return (APP_SCALES as readonly number[]).includes(percent)
    ? percent / 100
    : 1;
}

function readOsMedia(
  view: Pick<Window, "matchMedia"> | undefined,
): MediaQueryList | undefined {
  try {
    return typeof view?.matchMedia === "function"
      ? view.matchMedia(REDUCED_MOTION_QUERY)
      : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Applies the settings to the document root and keeps the motion level in
 * step with the OS state without a restart. Returns a function that removes
 * what it applied.
 */
export function applyPresentation(
  root: HTMLElement,
  settings: PresentationSettings,
  view: Pick<Window, "matchMedia"> | undefined = globalThis.window,
): () => void {
  root.style.setProperty(
    "--du-app-scale",
    String(scaleFactor(settings.appScale)),
  );
  const media = readOsMedia(view);
  const update = () => {
    root.dataset.duMotion = resolveMotionLevel(settings.motion, media?.matches);
  };
  update();
  media?.addEventListener?.("change", update);
  return () => {
    media?.removeEventListener?.("change", update);
    delete root.dataset.duMotion;
    root.style.removeProperty("--du-app-scale");
  };
}
