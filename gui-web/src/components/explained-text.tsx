import { useId, useRef } from "react";
import { HintPopup, useHint } from "./hint";
import "../theme/text-fit.css";
import "../theme/name-text.css";
import "../theme/explained-text.css";

/**
 * A short text, such as the label of a value, with an explanation of what it
 * stands for.
 *
 * The explanation shows beside the text while the pointer is on the text or
 * the text has keyboard focus. The pointer can move onto the explanation
 * without closing it, and Escape hides it until pointer and focus have left.
 *
 * The text is a tab stop with `role="note"`: it is read as the text it holds
 * and announces no action, because there is nothing to press. The explanation
 * is its accessible description. A visually hidden copy follows the text, so
 * a screen reader that reads line by line reaches it without focus too; the
 * copy is left out of a text selection.
 */
export function ExplainedText({
  children,
  explanation,
}: {
  children: string;
  explanation: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const descriptionId = useId();
  const hint = useHint(ref, true, true);
  return (
    <span className="du-explained">
      <span
        ref={ref}
        className="du-explained__text"
        role="note"
        tabIndex={0}
        aria-describedby={descriptionId}
        onPointerEnter={hint.enter}
        onPointerLeave={hint.leave}
      >
        {children}
      </span>
      <span id={descriptionId} className="du-visually-hidden">
        {explanation}
      </span>
      <HintPopup hint={hint} triggerRef={ref}>
        <span>{explanation}</span>
      </HintPopup>
    </span>
  );
}
