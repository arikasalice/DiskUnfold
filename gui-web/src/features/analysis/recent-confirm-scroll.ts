/** Shows the whole confirmation when possible; otherwise starts at its explanation. */
export function revealConfirmGroup(
  area: HTMLElement,
  group: HTMLElement,
): boolean {
  const root = document.scrollingElement ?? document.documentElement;
  let scroller: HTMLElement | null = area;
  while (scroller && scroller !== root) {
    const overflow = getComputedStyle(scroller).overflowY;
    if (
      (overflow === "auto" || overflow === "scroll") &&
      scroller.scrollHeight > scroller.clientHeight
    )
      break;
    scroller = scroller.parentElement;
  }
  if (!scroller) return false;
  const frame = scroller.getBoundingClientRect();
  const bounds = group.getBoundingClientRect();
  const isRoot = scroller === root;
  const top = isRoot ? 0 : frame.top + scroller.clientTop;
  const height = isRoot
    ? document.documentElement.clientHeight
    : scroller.clientHeight;
  const bottom = top + height;
  if (bounds.height > height || bounds.top < top)
    scroller.scrollTop += bounds.top - top;
  else if (bounds.bottom > bottom) scroller.scrollTop += bounds.bottom - bottom;
  return bounds.height > height;
}
