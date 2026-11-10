import { useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Action,
  Choice,
  ChoiceGroup,
  FolderDropSurface,
  Heading,
  LiveStatus,
  Notice,
  PathField,
  Progress,
  Text,
} from "../../components";
import { windowsPath } from "../../ipc";
import type { OsPath } from "../../ipc";
import type { useTargets } from "../../state/analysis-targets";
import { describeTargetCapacity } from "./target-capacity";

/** Presents discovered drives and the direct path entry defined for scope review. */
export function TargetPicker({
  targets,
  disabled,
  selected,
  choose,
  addPath,
  removePath,
  dropEnabled,
  dropError,
  pathDescription,
  reviewDescriptionId,
  feedback,
}: {
  targets: ReturnType<typeof useTargets>;
  disabled: boolean;
  selected: (OsPath | undefined)[];
  choose: (path: OsPath | undefined, index?: number) => void;
  addPath: () => void;
  removePath: (index: number) => void;
  dropEnabled: boolean;
  dropError: string | undefined;
  pathDescription: string;
  reviewDescriptionId: string | undefined;
  feedback: ReactNode;
}) {
  const fields = useRef(new Map<number, HTMLInputElement>());
  const focusIndex = useRef<number>(undefined);

  useLayoutEffect(() => {
    if (focusIndex.current === undefined) return;
    fields.current.get(focusIndex.current)?.focus();
    focusIndex.current = undefined;
  }, [selected.length]);

  function remove(index: number) {
    focusIndex.current = Math.min(index, selected.length - 2);
    removePath(index);
  }

  function registerField(index: number, input: HTMLInputElement | null) {
    if (input) fields.current.set(index, input);
    else fields.current.delete(index);
  }

  const pathField = (
    <PathField
      ref={(input) => registerField(0, input)}
      label="Windows folder path"
      description={pathDescription}
      placeholder={"C:\\Folder"}
      isDisabled={disabled}
      value={selected[0]?.display ?? ""}
      onChange={(path) => choose(path ? windowsPath(path) : undefined)}
    />
  );
  return (
    <div className="du-target-picker">
      <DrivePicker
        targets={targets}
        selected={selected[0]}
        disabled={disabled}
        choose={choose}
      />
      <section
        className="du-path-editor"
        aria-label="Folder paths"
        aria-describedby={reviewDescriptionId}
      >
        <div className="du-primary-path">
          {dropEnabled ? (
            <FolderDropSurface isDisabled={disabled}>
              {pathField}
            </FolderDropSurface>
          ) : (
            pathField
          )}
          <Action isDisabled={disabled} onPress={addPath}>
            Add path
          </Action>
        </div>
        {dropError && <Notice tone="error">{dropError}</Notice>}
        {selected.length > 1 && (
          <section className="du-scope-paths" aria-label="Additional paths">
            <Action isDisabled={disabled} onPress={() => remove(0)}>
              Remove primary path
            </Action>
            {selected.slice(1).map((path, index) => (
              <div className="du-scope-path" key={index}>
                <PathField
                  ref={(input) => registerField(index + 1, input)}
                  autoFocus
                  label={`Additional folder path ${index + 2}`}
                  description="Enter a local folder or drive path."
                  placeholder={"D:\\Folder"}
                  isDisabled={disabled}
                  value={path?.display ?? ""}
                  onChange={(value) =>
                    choose(value ? windowsPath(value) : undefined, index + 1)
                  }
                />
                <Action isDisabled={disabled} onPress={() => remove(index + 1)}>
                  {`Remove path ${index + 2}`}
                </Action>
              </div>
            ))}
          </section>
        )}
        {feedback}
      </section>
    </div>
  );
}

function DrivePicker({
  targets,
  selected,
  disabled,
  choose,
}: {
  targets: ReturnType<typeof useTargets>;
  selected: OsPath | undefined;
  disabled: boolean;
  choose: (path: OsPath | undefined) => void;
}) {
  const {
    targets: entries = [],
    probeJobRef,
    unavailableReason,
  } = targets.list ?? {};
  const error = targets.error ?? unavailableReason;
  const checkingCapacity = Boolean(probeJobRef) && !targets.finished;
  const notice =
    error === "os.unsupported"
      ? "Automatic drive listing is unavailable. You can still enter a folder path."
      : error === "os.call_slow"
        ? "Drive listing is taking longer than expected. Refresh later or enter a folder path."
        : error;
  const [showDriveDetails, setShowDriveDetails] = useState(false);
  const driveGroupId = useId();
  return (
    <section className="du-drive-picker" aria-label="Available drives">
      <div className="du-target-picker__header">
        <Heading level={3}>Drives</Heading>
        <Action
          variant="quiet"
          icon="refresh"
          aria-label="Refresh targets"
          isDisabled={targets.loading || disabled}
          onPress={() => void targets.refresh(true)}
        />
      </div>
      {targets.loading && (
        <div className="du-target-picker__loading">
          <LiveStatus>
            <Text>Finding drives…</Text>
          </LiveStatus>
          <Progress label="Loading targets" />
        </div>
      )}
      {notice && (
        <Notice
          tone={
            error === "os.unsupported" || error === "os.call_slow"
              ? "info"
              : "error"
          }
        >
          {notice}
        </Notice>
      )}
      {entries.length > 0 && (
        <>
          <ChoiceGroup
            id={driveGroupId}
            label="Choose a target"
            value={selected?.raw ?? null}
            isDisabled={disabled}
            onChange={(raw) =>
              choose(entries.find(({ path }) => path.raw === raw)?.path)
            }
          >
            {entries.map((target) => {
              const probe = targets.probes[target.path.raw];
              const access =
                probe?.access ??
                (targets.finished && target.access === "checking"
                  ? "unknown"
                  : target.access);
              const capacity = describeTargetCapacity(probe, checkingCapacity);
              const kind =
                target.kind?.replaceAll("_", " ") ?? "Type unavailable";
              return (
                <Choice
                  key={target.path.raw}
                  value={target.path.raw}
                  label={target.path.display}
                  icon="drive"
                  description={`${kind} · ${capacity.evidence}`}
                  summary={showDriveDetails ? undefined : capacity.summary}
                  availablePercent={capacity.availablePercent}
                  status={{
                    text: access,
                    tone: access === "accessible" ? "good" : "neutral",
                  }}
                />
              );
            })}
          </ChoiceGroup>
          <div className="du-drive-picker__details">
            <Action
              variant="quiet"
              aria-expanded={showDriveDetails}
              aria-controls={driveGroupId}
              onPress={() => setShowDriveDetails(!showDriveDetails)}
            >
              {showDriveDetails ? "Hide drive details" : "Drive details"}
            </Action>
          </div>
        </>
      )}
    </section>
  );
}
