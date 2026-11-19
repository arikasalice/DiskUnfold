import { useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  DetailDisclosure,
  NameText,
  Text,
  hasPathSeparator,
} from "../../components";

/**
 * Characters that take no visible space: format and control characters, such
 * as a zero-width space or a direction mark, and the Hangul fillers.
 */
const INVISIBLE = /[\p{Cf}\p{Cc}\u115F\u1160\u3164\uFFA0]/u;

/** True while the first element of `frame` is cut by its line clamp. */
function useClampCut(frame: RefObject<HTMLElement | null>, name: string) {
  const [cut, setCut] = useState(false);
  useLayoutEffect(() => {
    const shown = frame.current?.firstElementChild;
    if (!(shown instanceof HTMLElement)) return;
    const measure = () => setCut(shown.scrollHeight > shown.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(shown);
    return () => observer.disconnect();
  }, [frame, name]);
  return cut;
}

/**
 * The name row of the item details. A name that is a whole path, as a scope
 * root names itself, follows the path rule. "Full name" is offered only while
 * the two shown lines cut the name or the name holds characters that are not
 * visible; a name shown whole needs nothing more.
 */
export function InspectionName({
  name,
  fullLabel,
}: {
  name: string;
  fullLabel: string;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const cut = useClampCut(frame, name);
  const path = hasPathSeparator(name);
  return (
    <div ref={frame} className="du-inspection-name">
      <Text>
        <NameText name={name} path={path} />
      </Text>
      {(cut || INVISIBLE.test(name)) && (
        <DetailDisclosure title={fullLabel}>
          <Text focusable>
            <NameText name={name} path={path} />
          </Text>
        </DetailDisclosure>
      )}
    </div>
  );
}
