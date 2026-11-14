import { createContext, useContext, useRef } from "react";
import type { KeyboardEvent } from "react";

type CompositionState = "idle" | "composing" | "just-ended";

/** How one Enter key press is classified for a text entry. */
export type ImeKeyState = "composing" | "just-ended" | "unknown" | "idle";

/**
 * Whether an Enter that arrives outside composition may request an action.
 *
 * `run` lets one Enter run the entry's action (for example a search).
 * `never` is for approval entries: Enter there only edits text.
 */
export type EnterPolicy = "run" | "never";

/** What the entry does with a key press: pass it on, let the IME act, or consume it. */
export type ImeOutcome = "pass" | "ime" | "swallow";

type ImeRow = Readonly<{
  /** Stable row name used by tests and reviews. */
  id: string;
  state: ImeKeyState;
  key: "Enter" | "Escape";
  /** The row applies to every policy when omitted. */
  policy?: EnterPolicy;
  outcome: ImeOutcome;
}>;

/**
 * The IME decision table. It is the only place that turns composition state
 * into a decision, so no screen re-implements it.
 *
 * `ime` stops the key from reaching screen handlers and leaves it to the IME
 * (a composing Escape only cancels the composition). `swallow` also cancels
 * the key's default action. The first matching row wins and no match passes
 * the key on. A button needs no row: Space, Enter and a click reach its press
 * handler once each.
 */
export const IME_DECISION_TABLE: readonly ImeRow[] = [
  { id: "composing-enter", state: "composing", key: "Enter", outcome: "ime" },
  { id: "composing-escape", state: "composing", key: "Escape", outcome: "ime" },
  {
    id: "just-ended-enter",
    state: "just-ended",
    key: "Enter",
    outcome: "swallow",
  },
  {
    id: "just-ended-escape",
    state: "just-ended",
    key: "Escape",
    outcome: "ime",
  },
  { id: "unknown-enter", state: "unknown", key: "Enter", outcome: "swallow" },
  { id: "unknown-escape", state: "unknown", key: "Escape", outcome: "ime" },
  {
    id: "approval-enter",
    state: "idle",
    key: "Enter",
    policy: "never",
    outcome: "swallow",
  },
  {
    id: "idle-enter-run",
    state: "idle",
    key: "Enter",
    policy: "run",
    outcome: "pass",
  },
];

type ImeSignals = {
  isComposing?: boolean | undefined;
  keyCode?: number | undefined;
  key?: string | undefined;
};

/** True while the IME is processing this key (flag, legacy 229 or "Process"). */
export function isImeProcessingKey(event: ImeSignals): boolean {
  return (
    event.isComposing === true ||
    event.keyCode === 229 ||
    event.key === "Process"
  );
}

/**
 * Classifies a key press from the tracked composition state and the event's
 * own signals. A press whose composition flag and key code are both absent
 * cannot be judged and is `unknown`.
 */
export function classifyImeKeyState(
  tracked: CompositionState,
  event: ImeSignals,
): ImeKeyState {
  if (tracked === "composing" || isImeProcessingKey(event)) return "composing";
  if (tracked === "just-ended") return "just-ended";
  const known =
    event.isComposing === true ||
    event.isComposing === false ||
    (typeof event.keyCode === "number" && event.keyCode !== 0);
  return known ? "idle" : "unknown";
}

/** Looks up the table row for one key press; `pass` when no row applies. */
export function decideImeKey(
  state: ImeKeyState,
  key: string,
  policy: EnterPolicy,
): { id: string | undefined; outcome: ImeOutcome } {
  const row = IME_DECISION_TABLE.find(
    (candidate) =>
      candidate.state === state &&
      candidate.key === key &&
      (candidate.policy === undefined || candidate.policy === policy),
  );
  return { id: row?.id, outcome: row?.outcome ?? "pass" };
}

const EnterPolicyContext = createContext<EnterPolicy>("run");

/**
 * Provides the Enter policy for every text entry below it. A confirmation
 * dialog provides `never` so no entry inside it can submit with Enter.
 */
export const EnterPolicyProvider = EnterPolicyContext.Provider;

/** Keys that never end a just-ended mark: they carry no text or action. */
const MODIFIER_KEYS = new Set([
  "Alt",
  "AltGraph",
  "CapsLock",
  "Control",
  "Meta",
  "NumLock",
  "Shift",
]);

/**
 * Keeps IME composition keys from becoming application actions.
 *
 * `onEnter` runs once for an Enter that the table classifies as `idle-enter-run`
 * and is skipped for held-key repeats. The returned handlers go on the input.
 *
 * The mark that follows a composition has no timer. WebView2 and WKWebView
 * deliver `compositionend` inside the key press that ends the composition,
 * but WebKitGTK with ibus or fcitx may deliver it and the Enter keydown in
 * different tasks, so any delay could expire before that Enter arrives. The
 * mark stays until one of these happens: the release of the key that ended the
 * composition, a second separate Enter press, another non-modifier key press,
 * a new composition, or blur. The first Enter press while the mark is set is
 * consumed (row `just-ended-enter`), so at most one Enter is lost.
 */
export function useImeComposition(
  onCompositionChange?: (composing: boolean) => void,
  options: { onEnter?: () => void; enter?: EnterPolicy } = {},
) {
  const state = useRef<CompositionState>("idle");
  // True once the mark has consumed an Enter press that has not been released.
  const enterConsumed = useRef(false);
  // The physical key of the latest key press, to pair it with its release.
  const lastKeyCode = useRef<string | undefined>(undefined);
  const contextPolicy = useContext(EnterPolicyContext);
  const policy = options.enter ?? contextPolicy;

  function setState(next: CompositionState) {
    state.current = next;
    enterConsumed.current = false;
  }

  function onCompositionStart() {
    setState("composing");
    onCompositionChange?.(true);
  }

  function onCompositionUpdate() {
    setState("composing");
    onCompositionChange?.(true);
  }

  function onCompositionEnd() {
    setState("just-ended");
    onCompositionChange?.(false);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    const processing = isImeProcessingKey(event.nativeEvent);
    const modifier = MODIFIER_KEYS.has(event.key);
    if (!modifier) lastKeyCode.current = event.nativeEvent.code || undefined;
    if (event.key !== "Enter" && event.key !== "Escape") {
      if (!processing && !modifier) setState("idle");
      return;
    }
    // A second, separate Enter press means the first one's release was lost.
    if (
      state.current === "just-ended" &&
      enterConsumed.current &&
      event.key === "Enter" &&
      !event.repeat &&
      !processing
    )
      setState("idle");

    const keyState = classifyImeKeyState(state.current, event.nativeEvent);
    const { outcome } = decideImeKey(keyState, event.key, policy);
    if (keyState === "just-ended") {
      // The consumed Enter keeps the mark until its release; Escape ends it now.
      if (event.key === "Enter") enterConsumed.current = true;
      else setState("idle");
    }
    if (outcome === "pass") {
      if (event.key === "Enter" && !event.repeat) options.onEnter?.();
      return;
    }
    event.stopPropagation();
    if (outcome === "swallow") event.preventDefault();
  }

  function onKeyUp(event: KeyboardEvent<HTMLInputElement>) {
    if (state.current !== "just-ended" || MODIFIER_KEYS.has(event.key)) return;
    // The release of some other key does not end the mark.
    const code = event.nativeEvent.code;
    if (code && lastKeyCode.current && code !== lastKeyCode.current) return;
    setState("idle");
  }

  function onBlur() {
    setState("idle");
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
