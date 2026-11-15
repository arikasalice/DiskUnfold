import { useId, useRef } from "react";
import { HintPopup, useHint } from "./hint";
import "../theme/name-text.css";
import "../theme/explained-text.css";

/**
 * A short text, such as the label of a value, with an explanation of what it
 * stands for. A dotted underline and the help cursor show that it has one.
 *
 * The explanation shows beside the text while the pointer is on the text or
 * the text has keyboard focus. The pointer can move onto the explanation
 * without closing it, and Escape hides it until pointer and focus have left.
 *
 * The text is a tab stop with `role="note"`: it announces no action, because
 * there is nothing to press. A note takes no name from its content, so the
 * text names itself, and the explanation is its accessible description. The
 * description comes from an element that is not rendered: it adds nothing to
 * the name of an element around the text and is never read a second time.
 */
export function ExplainedText({
  children,
  explanation,
}: {
  children: string;
  explanation: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const textId = useId();
  const descriptionId = useId();
  const hint = useHint(ref, true);
  return (
    <>
      <span
        ref={ref}
        id={textId}
        className="du-explained"
        role="note"
        tabIndex={0}
        aria-labelledby={textId}
        aria-describedby={descriptionId}
        onPointerEnter={hint.enter}
        onPointerLeave={hint.leave}
      >
        {children}
      </span>
      <span id={descriptionId} hidden>
        {explanation}
      </span>
      <HintPopup hint={hint} triggerRef={ref}>
        <span>{explanation}</span>
      </HintPopup>
    </>
  );
}
