import { useEffect, useRef } from "react";
import type { KeyboardEvent } from "react";

type CompositionState = "idle" | "composing" | "just-ended";

/** Keeps IME completion keys from bubbling into application actions. */
export function useImeComposition(
  onCompositionChange?: (composing: boolean) => void,
) {
  const state = useRef<CompositionState>("idle");
  const resetTimer = useRef<number | undefined>(undefined);

  function clearResetTimer() {
    if (resetTimer.current !== undefined) {
      window.clearTimeout(resetTimer.current);
      resetTimer.current = undefined;
    }
  }

  useEffect(
    () => () => {
      if (resetTimer.current !== undefined)
        window.clearTimeout(resetTimer.current);
    },
    [],
  );

  function onCompositionStart() {
    clearResetTimer();
    state.current = "composing";
    onCompositionChange?.(true);
  }

  function onCompositionUpdate() {
    clearResetTimer();
    state.current = "composing";
    onCompositionChange?.(true);
  }

  function onCompositionEnd() {
    clearResetTimer();
    state.current = "just-ended";
    onCompositionChange?.(false);
    resetTimer.current = window.setTimeout(() => {
      state.current = "idle";
      resetTimer.current = undefined;
    }, 0);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key !== "Enter" && event.key !== "Escape") return;

    const stateIsComposing = state.current === "composing";
    const eventIsComposing = event.nativeEvent.isComposing === true;
    const legacyImeKey = event.nativeEvent.keyCode === 229;
    const justEnded = state.current === "just-ended";
    const eventIsKnown =
      event.nativeEvent.isComposing === true ||
      event.nativeEvent.isComposing === false ||
      (typeof event.nativeEvent.keyCode === "number" &&
        event.nativeEvent.keyCode !== 0);
    const composing = stateIsComposing || eventIsComposing || legacyImeKey;

    if (!composing && !justEnded && eventIsKnown) return;

    event.stopPropagation();
    // Let the IME act while composing; consume stale or unknown Enter.
    if (event.key === "Enter" && !composing) event.preventDefault();
  }

  function onKeyUp() {
    if (state.current !== "just-ended") return;
    clearResetTimer();
    state.current = "idle";
  }

  function onBlur() {
    clearResetTimer();
    state.current = "idle";
    onCompositionChange?.(false);
  }

  return {
    onBlur,
    onCompositionEnd,
    onCompositionStart,
    onCompositionUpdate,
    onKeyDown,
    onKeyUp,
  };
}
