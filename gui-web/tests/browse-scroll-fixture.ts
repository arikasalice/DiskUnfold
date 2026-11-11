import { beforeEach, vi } from "vitest";

let top = 0;
/** Model only the list viewport: document movement is deliberately unrelated. */
export const listScroll = {
  get scrollY() {
    return top;
  },
  scrollBy(_x: number, y: number) {
    top += y;
  },
  scrollTo(_x: number, y: number) {
    top = y;
  },
};
export function listRoot() {
  const root = document.querySelector<HTMLElement>(".du-browse-grid-frame");
  if (!root) throw new Error("Browse list is not mounted");
  return root;
}
beforeEach(() => {
  top = 0;
  vi.spyOn(HTMLElement.prototype, "scrollTop", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("du-browse-grid-frame")
        ? listScroll.scrollY
        : 0;
    },
  );
  vi.spyOn(HTMLElement.prototype, "scrollTop", "set").mockImplementation(
    function (this: HTMLElement, value: number) {
      if (this.classList.contains("du-browse-grid-frame"))
        listScroll.scrollBy(0, value - listScroll.scrollY);
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("du-browse-grid-frame") ? 480 : 0;
    },
  );
});
