import { useEffect, useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";

/**
 * Keeps the block size its content last had while `holding` is on, so
 * content replaced by a shorter placeholder does not move what follows it.
 *
 * The size is recorded whenever the content changes size while not holding,
 * including when a part inside it expands. Holding starts from the size the
 * content had just before; it ends by letting the new content size itself.
 */
export function HeldHeight({
  holding,
  children,
}: {
  holding: boolean;
  children: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const last = useRef(0);
  const held = useRef(holding);
  useLayoutEffect(() => {
    const node = box.current;
    if (!node) return;
    held.current = holding;
    if (holding) {
      node.style.minHeight = `${last.current}px`;
    } else {
      node.style.minHeight = "";
      last.current = node.getBoundingClientRect().height;
    }
  });
  useEffect(() => {
    const node = box.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (!held.current) last.current = node.getBoundingClientRect().height;
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return <div ref={box}>{children}</div>;
}
