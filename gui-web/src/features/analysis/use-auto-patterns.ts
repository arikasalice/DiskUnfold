import { useSyncExternalStore } from "react";

const AUTO_PATTERNS = "(forced-colors: active), (prefers-contrast: more)";

/**
 * Whether chart patterns draw whatever the pattern switch says (F06 README 4.1): in
 * forced colours or when the system asks for more contrast. The card's type bar has no
 * switch of its own, so it follows this alone.
 */
export function useAutoPatterns() {
  return useSyncExternalStore(
    (changed) => {
      const query = window.matchMedia?.(AUTO_PATTERNS);
      query?.addEventListener("change", changed);
      return () => query?.removeEventListener("change", changed);
    },
    () => window.matchMedia?.(AUTO_PATTERNS).matches ?? false,
    () => false,
  );
}
