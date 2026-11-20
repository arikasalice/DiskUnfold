// happy-dom 21 dispatches "selectionchange" synchronously from inside every
// Selection change, and Selection.collapse() always installs a new Range, so
// collapsing to the same position fires again. React Aria's date segments
// collapse the selection on every selectionchange while focused, so the two
// recurse until the stack overflows ("Maximum call stack size exceeded" from
// useDateSegment). Browsers schedule the event as a task (Selection API,
// "schedule a selectionchange event") and fire nothing when the selection did
// not move: in Edge 157 the same pattern fires once on focus and stops.
//
// This setup keeps top-level dispatches synchronous, as tests rely on, and
// treats a dispatch made by a listener during another selectionchange like a
// browser would: dropped when the selection did not move, otherwise queued.
type Boundary = readonly [Node | null, number, Node | null, number];

function boundary(document: Document): Boundary {
  const selection = document.getSelection();
  return [
    selection?.anchorNode ?? null,
    selection?.anchorOffset ?? 0,
    selection?.focusNode ?? null,
    selection?.focusOffset ?? 0,
  ];
}

// happy-dom gives each window its own Document subclass, so the wrapper goes
// on the test document itself, which is the object Selection dispatches on.
function install(target: Document) {
  const dispatch = target.dispatchEvent.bind(target);
  let handling: { event: Event; boundary: Boundary } | undefined;
  target.dispatchEvent = (event: Event) => {
    if (event.type !== "selectionchange") return dispatch(event);
    // happy-dom re-enters dispatchEvent with the same event for its phases.
    if (handling?.event === event) return dispatch(event);
    const current = boundary(target);
    if (handling) {
      const start = handling.boundary;
      const moved = current.some((value, index) => value !== start[index]);
      if (moved) setTimeout(() => dispatch(new Event("selectionchange")));
      return true;
    }
    handling = { event, boundary: current };
    try {
      return dispatch(event);
    } finally {
      handling = undefined;
    }
  };
}

// Test files that run in the node environment have no document.
if (typeof document !== "undefined") install(document);
