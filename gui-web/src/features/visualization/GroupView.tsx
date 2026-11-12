import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { RefObject } from "react";
import { Action, LiveStatus, Notice } from "../../components";
import { GroupMembersPresenter } from "./GroupMembersPresenter";
import type { GroupController, GroupFrame } from "./group-controller";
import type { ModelResponse, VizSelection } from "./model-adapter";
import { visualizationShapeRef } from "./model-adapter";
import type { VizText } from "./model-text";
import type { GroupOrigin } from "./group-navigation";

export function GroupView({
  controller,
  frame,
  selection,
  origin = null,
  returnTargetRef,
  text,
}: {
  controller: GroupController;
  frame: ModelResponse | null;
  selection: VizSelection | null;
  origin?: GroupOrigin | null;
  returnTargetRef?: { current: HTMLElement | null };
  text: VizText;
}) {
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
  );
  const openButton = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const returnNotice = useRef<HTMLDivElement>(null);
  const visibleRef = useRef(false);
  const closingByUser = useRef(false);
  const [returnUnavailable, setReturnUnavailable] = useState(false);
  const selected = selectedShape(frame, selection);
  const current: GroupFrame | null = frame
    ? {
        modelRef: frame.model.modelRef,
        resultRevision: frame.resultRevision,
        observedAt: frame.observedAt,
        completeness: frame.completeness,
      }
    : null;
  const matches = sameFrame(current, state.frame);
  useEffect(
    () => synchronizeFrame(controller, current),
    [
      controller,
      current?.modelRef,
      current?.resultRevision,
      current?.observedAt,
      current?.completeness,
    ],
  );
  useEffect(() => () => controller.close(), [controller]);
  const originMatches = isOriginForOpenGroup(
    origin,
    current,
    state.groupRef,
    frame,
  );
  const sameGroup = isCurrentOpenGroup(
    selected,
    state.groupRef,
    origin,
    originMatches,
  );
  useEffect(() => {
    if (state.status !== "closed" && !sameGroup) controller.close();
  }, [controller, sameGroup, state.status]);
  const visible = isGroupViewVisible(matches, sameGroup, state.status);
  const validOrigin = getConnectedOrigin(origin, originMatches);
  const returnTitle = getReturnTitle(
    originMatches ? (origin?.returnTarget ?? null) : null,
    returnTargetRef?.current ?? null,
    groupTitle(origin, text),
  );
  const groupHadFocusBeforeCommit = Boolean(
    panel.current?.contains(document.activeElement) ||
    status.current === document.activeElement,
  );
  useLayoutEffect(() => {
    const wasVisible = visibleRef.current;
    visibleRef.current = visible;
    if (
      wasVisible &&
      !visible &&
      groupHadFocusBeforeCommit &&
      !closingByUser.current
    )
      setReturnUnavailable(true);
    closingByUser.current = false;
  }, [groupHadFocusBeforeCommit, visible]);
  useEffect(() => {
    focusGroupStatus(visible, validOrigin, openButton.current, status.current);
  }, [validOrigin, visible]);
  useLayoutEffect(() => {
    focusReturnTitle(
      returnUnavailable,
      returnTargetRef?.current ?? null,
      returnNotice.current,
    );
  }, [returnTargetRef, returnUnavailable]);
  useEffect(() => {
    if (visible) setReturnUnavailable(false);
  }, [visible]);
  return (
    <>
      {selected && current && (
        <Action
          ref={openButton}
          onPress={() => {
            void openSelectedGroup(controller, current, selected);
          }}
        >
          {text.message("viewGroupMembers")}
        </Action>
      )}
      {visible && (
        <>
          <LiveStatus ref={status} focusTarget>
            {text.message("groupMembers")}
          </LiveStatus>
          <GroupContents
            state={state}
            controller={controller}
            text={text}
            title={groupTitle(origin, text)}
            onClose={() => {
              closingByUser.current = true;
              closeGroupView(
                controller,
                validOrigin,
                getOriginalViewTarget(origin, originMatches),
                returnTargetRef?.current ?? null,
                setReturnUnavailable,
              );
            }}
            panelRef={panel}
          />
        </>
      )}
      {!visible && returnUnavailable && (
        <div
          ref={returnNotice}
          className="du-review-stack"
          tabIndex={-1}
          aria-label={returnTitle}
        >
          <Notice>{text.message("groupTargetUnavailable")}</Notice>
        </div>
      )}
    </>
  );
}

function synchronizeFrame(
  controller: GroupController,
  frame: GroupFrame | null,
) {
  if (!frame) controller.close();
  else controller.replaceFrame(frame);
}

function groupTitle(origin: GroupOrigin | null, text: VizText) {
  return origin?.groupLabel?.trim() || text.message("otherMembers");
}

function getReturnTitle(
  originalTarget: HTMLElement | null,
  currentTarget: HTMLElement | null,
  fallback: string,
) {
  const target = originalTarget?.isConnected ? originalTarget : currentTarget;
  if (!target) return fallback;
  const label = target.getAttribute("aria-label")?.trim();
  if (label) return label;
  const labelledBy = target.getAttribute("aria-labelledby");
  if (labelledBy) {
    const text = labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent?.trim())
      .filter(Boolean)
      .join(" ");
    if (text) return text;
  }
  const heading = target.querySelector("h1, h2, h3, h4, h5, h6");
  return heading?.textContent?.trim() || target.textContent?.trim() || fallback;
}

function isOriginForOpenGroup(
  origin: GroupOrigin | null,
  frame: GroupFrame | null,
  groupRef: string | null,
  model: ModelResponse | null,
) {
  if (
    !origin ||
    !frame ||
    origin.modelRef !== frame.modelRef ||
    origin.resultRevision !== frame.resultRevision ||
    origin.observedAt !== frame.observedAt ||
    origin.completeness !== frame.completeness ||
    origin.groupRef !== groupRef
  )
    return false;
  if (origin.kind === "row") return true;
  const matches = model?.model.shapes.filter(
    (shape, index) =>
      shape.groupRef === origin.groupRef &&
      visualizationShapeRef(shape.shapeRef, index) === origin.shapeRef,
  );
  return matches?.length === 1;
}

function isOpenGroupSelected(
  selected: ReturnType<typeof selectedShape>,
  groupRef: string | null,
) {
  return !!groupRef && selected?.groupRef === groupRef;
}

function isCurrentOpenGroup(
  selected: ReturnType<typeof selectedShape>,
  groupRef: string | null,
  origin: GroupOrigin | null,
  originMatches: boolean,
) {
  return (
    isOpenGroupSelected(selected, groupRef) ||
    (origin?.kind === "row" && originMatches)
  );
}

function isGroupViewVisible(
  matches: boolean,
  sameGroup: boolean,
  status: ReturnType<GroupController["getSnapshot"]>["status"],
) {
  return matches && sameGroup && status !== "closed";
}

function getConnectedOrigin(
  origin: GroupOrigin | null,
  originMatches: boolean,
) {
  if (!originMatches || !origin?.element.isConnected) return null;
  if (origin.returnTarget && !origin.returnTarget.contains(origin.element))
    return null;
  return origin.element;
}

function focusGroupStatus(
  visible: boolean,
  origin: HTMLElement | null,
  openButton: HTMLButtonElement | null,
  status: HTMLDivElement | null,
) {
  if (
    visible &&
    (document.activeElement === origin || document.activeElement === openButton)
  )
    status?.focus();
}

function focusReturnTitle(
  returnUnavailable: boolean,
  currentTarget: HTMLElement | null,
  fallbackTarget: HTMLElement | null,
) {
  if (!returnUnavailable) return;
  if (focusElement(currentTarget)) return;
  focusElement(fallbackTarget);
}

function getOriginalViewTarget(
  origin: GroupOrigin | null,
  originMatches: boolean,
) {
  return originMatches ? (origin?.returnTarget ?? null) : null;
}

function focusElement(target: HTMLElement | null) {
  if (!target?.isConnected) return false;
  target.focus();
  return document.activeElement === target;
}

function openSelectedGroup(
  controller: GroupController,
  frame: GroupFrame,
  selected: NonNullable<ReturnType<typeof selectedShape>>,
) {
  return controller.open(frame, {
    groupRef: selected.groupRef!,
    shape: selected,
  });
}

function closeGroupView(
  controller: GroupController,
  origin: HTMLElement | null,
  originalViewTarget: HTMLElement | null,
  currentViewTarget: HTMLElement | null,
  setReturnUnavailable: (value: boolean) => void,
) {
  controller.close();
  if (focusElement(origin)) return;
  if (!focusElement(originalViewTarget)) focusElement(currentViewTarget);
  setReturnUnavailable(true);
}

function selectedShape(
  frame: ModelResponse | null,
  selection: VizSelection | null,
) {
  if (
    !frame ||
    selection?.kind !== "group" ||
    selection.modelRef !== frame.model.modelRef ||
    selection.resultRevision !== frame.resultRevision
  )
    return undefined;
  const matches = frame.model.shapes.filter(
    (shape) => shape.groupRef === selection.groupRef,
  );
  return matches.length === 1 ? matches[0] : undefined;
}
function sameFrame(a: GroupFrame | null, b: GroupFrame | null) {
  return (
    !!a &&
    !!b &&
    a.modelRef === b.modelRef &&
    a.resultRevision === b.resultRevision &&
    a.observedAt === b.observedAt &&
    a.completeness === b.completeness
  );
}
function GroupContents({
  state,
  controller,
  text,
  title,
  onClose,
  panelRef,
}: {
  state: ReturnType<GroupController["getSnapshot"]>;
  controller: GroupController;
  text: VizText;
  title: string;
  onClose: () => void;
  panelRef: RefObject<HTMLDivElement | null>;
}) {
  return (
    <div ref={panelRef}>
      <GroupMembersPresenter
        title={title}
        returnLabel={text.message("backToVisualization")}
        rows={state.page?.response.rows}
        {...(state.rangeLabel ? { rangeLabel: state.rangeLabel } : {})}
        {...(state.groupTotal
          ? {
              groupTotalLabel: text.size(state.groupTotal),
              sizeBasisLabel: text.key(state.groupTotal.basis),
            }
          : {})}
        {...(state.frame
          ? { observedAtLabel: text.observedAt(state.frame.observedAt) }
          : {})}
        {...(state.sizeRange?.kind === "known"
          ? {
              sizeRangeLabel: `${text.size(state.sizeRange.min)} – ${text.size(state.sizeRange.max)}`,
            }
          : {})}
        status={state.status === "closed" ? "loading" : state.status}
        {...(state.failure
          ? {
              errorMessage:
                state.failure === "stale"
                  ? text.message("groupResultStale")
                  : state.failure === "missing"
                    ? text.message("groupMissing")
                    : state.failure === "invalid"
                      ? text.message("groupResponseMismatch")
                      : text.message("loadRangeFailed"),
            }
          : {})}
        canRetry={state.canRetry}
        hasPrevious={state.hasPrevious}
        hasNext={state.hasNext}
        text={text}
        onClose={onClose}
        onPrevious={() => {
          void controller.previous();
        }}
        onNext={() => {
          void controller.next();
        }}
        onRetry={() => {
          void controller.retry();
        }}
      />
    </div>
  );
}
