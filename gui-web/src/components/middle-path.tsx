import { useRef } from "react";
import { HintPopup, useHint } from "./hint";

/** Characters kept at each end of a shortened path. */
const EDGE = 24;

/**
 * A path that keeps its start and end when long ("C:\Users\…\docs\video"),
 * with the whole path as its explanation on pointer or focus. Assistive
 * technology reads the whole path.
 */
export function MiddlePath({ path }: { path: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const shortened = path.length > EDGE * 2 + 1;
  const hint = useHint(ref, shortened);
  if (!shortened) return <span className="du-middle-path">{path}</span>;
  return (
    <>
      <span
        ref={ref}
        className="du-middle-path"
        onPointerEnter={hint.enter}
        onPointerLeave={hint.leave}
      >
        <span aria-hidden="true">
          {`${path.slice(0, EDGE)}…${path.slice(-EDGE)}`}
        </span>
        <span className="du-visually-hidden">{path}</span>
      </span>
      <HintPopup hint={hint} triggerRef={ref}>
        {path}
      </HintPopup>
    </>
  );
}
