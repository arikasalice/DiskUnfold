import { useEffect, useRef, useSyncExternalStore } from "react";
import { Action, LiveStatus } from "../../components";
import { GroupMembersPresenter } from "./GroupMembersPresenter";
import type { GroupController, GroupFrame } from "./group-controller";
import type { ModelResponse, VizSelection } from "./model-adapter";
import type { VizText } from "./model-text";

export function GroupView({
  controller,
  frame,
  selection,
  text,
}: {
  controller: GroupController;
  frame: ModelResponse | null;
  selection: VizSelection | null;
  text: VizText;
}) {
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
  );
  const openButton = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
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
  useEffect(() => {
    if (!current) controller.close();
    else controller.replaceFrame(current);
  }, [
    controller,
    current?.modelRef,
    current?.resultRevision,
    current?.observedAt,
    current?.completeness,
  ]);
  useEffect(() => () => controller.close(), [controller]);
  const sameGroup = selected?.groupRef === state.groupRef && !!state.groupRef;
  useEffect(() => {
    if (state.status !== "closed" && !sameGroup) controller.close();
  }, [controller, sameGroup, state.status]);
  const visible = matches && sameGroup && state.status !== "closed";
  useEffect(() => {
    if (visible) status.current?.focus({ preventScroll: true });
  }, [visible]);
  return (
    <>
      {selected && current && (
        <Action
          ref={openButton}
          onPress={() => {
            void controller.open(current, {
              groupRef: selected.groupRef!,
              shape: selected,
            });
          }}
        >
          View group members
        </Action>
      )}
      {visible && (
        <>
          <LiveStatus ref={status} focusTarget>
            Group members
          </LiveStatus>
          <GroupContents
            state={state}
            controller={controller}
            text={text}
            onClose={() => {
              controller.close();
              openButton.current?.focus();
            }}
          />
        </>
      )}
    </>
  );
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
  return frame.model.shapes.find(
    (shape) => shape.groupRef === selection.groupRef,
  );
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
  onClose,
}: {
  state: ReturnType<GroupController["getSnapshot"]>;
  controller: GroupController;
  text: VizText;
  onClose: () => void;
}) {
  return (
    <GroupMembersPresenter
      returnLabel="Back to visualization"
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
                ? "This result is no longer available. Return to the visualization to refresh it."
                : state.failure === "missing"
                  ? "This group is no longer available. Return to the visualization."
                  : state.failure === "invalid"
                    ? "The group response did not match this visualization."
                    : "Could not load this range.",
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
  );
}
