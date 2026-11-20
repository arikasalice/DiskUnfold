import { useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, RefObject } from "react";
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
import { nativePath } from "../../ipc";
import type { OsPath } from "../../ipc";
import type { useTargets } from "../../state/analysis-targets";
import { useRegionalFormat } from "../../i18n/regional-format";
import { describeTargetCapacity } from "./target-capacity";
import { accessLabel, defaultListing, shownAccess } from "./target-access";
import type { ScopeReview } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";

/** The latest scope review as far as it checks a selected target (F01 #266). */
export type TargetReview = {
  reviewing: boolean;
  targets: ScopeReview["targets"] | undefined;
  openPrivacySettings: () => void;
};

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
  isInitial,
  onPathCompositionChange,
  primaryPathRef,
  review,
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
  isInitial: boolean;
  onPathCompositionChange: (composing: boolean) => void;
  primaryPathRef?: RefObject<HTMLInputElement | null>;
  review: TargetReview;
}) {
  const text = useLocaleText();
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
      ref={(input) => {
        registerField(0, input);
        if (primaryPathRef) primaryPathRef.current = input;
      }}
      autoFocus={isInitial}
      label={text("ui.analysis.folder_path", "Folder path")}
      description={pathDescription}
      placeholder={text(
        "ui.analysis.path_example_first",
        "e.g. C:\\Users\\Minseo\\Documents",
      )}
      isDisabled={disabled}
      onCompositionChange={onPathCompositionChange}
      value={selected[0]?.display ?? ""}
      onChange={(path) => choose(path ? nativePath(path) : undefined)}
    />
  );
  const drivePicker = (
    <DrivePicker
      key="drives"
      isInitial={isInitial}
      targets={targets}
      selected={selected[0]}
      disabled={disabled}
      choose={choose}
      review={review}
    />
  );
  const pathEditor = (
    <section
      key="path"
      className="du-path-editor"
      aria-label={text("ui.analysis.folder_paths", "Folder paths")}
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
          {text("ui.analysis.add_path", "Add path")}
        </Action>
      </div>
      {dropError && <Notice tone="error">{dropError}</Notice>}
      {selected.length > 1 && (
        <section
          className="du-scope-paths"
          aria-label={text("ui.analysis.additional_paths", "Additional paths")}
        >
          <Action isDisabled={disabled} onPress={() => remove(0)}>
            {text("ui.analysis.remove_primary", "Remove primary path")}
          </Action>
          {selected.slice(1).map((path, index) => (
            <div className="du-scope-path" key={index}>
              <PathField
                ref={(input) => registerField(index + 1, input)}
                autoFocus
                label={text(
                  "ui.analysis.additional_path",
                  "Additional folder path {number}",
                  { number: BigInt(index + 2) },
                )}
                description={text(
                  "ui.analysis.path_hint",
                  "Enter a local folder or drive path.",
                )}
                placeholder={text(
                  "ui.analysis.path_example_more",
                  "e.g. D:\\Photos",
                )}
                isDisabled={disabled}
                onCompositionChange={onPathCompositionChange}
                value={path?.display ?? ""}
                onChange={(value) =>
                  choose(value ? nativePath(value) : undefined, index + 1)
                }
              />
              <Action isDisabled={disabled} onPress={() => remove(index + 1)}>
                {text("ui.analysis.remove_path", "Remove path {number}", {
                  number: BigInt(index + 2),
                })}
              </Action>
            </div>
          ))}
        </section>
      )}
      {feedback}
    </section>
  );
  return (
    <div className="du-target-picker">
      {isInitial ? [pathEditor, drivePicker] : [drivePicker, pathEditor]}
    </div>
  );
}

function targetListingNotice(
  error: string | null | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  if (error === "os.unsupported")
    return text(
      "ui.analysis.drives_unsupported",
      "Automatic drive listing is unavailable. You can still enter a folder path.",
    );
  if (error === "os.call_slow")
    return text(
      "ui.analysis.drives_slow",
      "Drive listing is taking longer than expected. Refresh later or enter a folder path.",
    );
  if (
    typeof error === "string" &&
    /^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$/.test(error)
  )
    return text(
      "ui.analysis.drives_unavailable",
      "Drive listing is unavailable. You can still enter a folder path.",
    );
  return error ?? undefined;
}

function DrivePicker({
  isInitial,
  targets,
  selected,
  disabled,
  choose,
  review,
}: {
  isInitial: boolean;
  targets: ReturnType<typeof useTargets>;
  selected: OsPath | undefined;
  disabled: boolean;
  choose: (path: OsPath | undefined) => void;
  review: TargetReview;
}) {
  const text = useLocaleText();
  const regional = useRegionalFormat();
  const {
    targets: entries = [],
    probeJobRef,
    unavailableReason,
  } = targets.list ?? {};
  const error = targets.error ?? unavailableReason;
  const checkingCapacity = Boolean(probeJobRef) && !targets.finished;
  const notice = targetListingNotice(error, text);
  const [showDriveDetails, setShowDriveDetails] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const { listed, hidden } = defaultListing(entries, selected, showHidden);
  let deniedNetworkVolume = false;
  const driveGroupId = useId();
  return (
    <section
      className="du-drive-picker"
      aria-label={text("ui.analysis.available_drives", "Available drives")}
    >
      <div className="du-target-picker__header">
        <Heading level={3}>{text("ui.analysis.drives", "Drives")}</Heading>
        <div className="du-drive-picker__actions">
          {isInitial && entries.length > 0 && (
            <Action
              variant="quiet"
              aria-expanded={showDriveDetails}
              aria-controls={driveGroupId}
              onPress={() => setShowDriveDetails(!showDriveDetails)}
            >
              {showDriveDetails
                ? text("ui.analysis.hide_drive_details", "Hide drive details")
                : text("ui.analysis.drive_details", "Drive details")}
            </Action>
          )}
          <Action
            variant="quiet"
            icon="refresh"
            aria-label={text("ui.analysis.refresh_targets", "Refresh targets")}
            isDisabled={disabled}
            isPending={targets.loading}
            onPress={() => void targets.refresh(true)}
          />
        </div>
      </div>
      {targets.loading && (
        <div className="du-target-picker__loading">
          <LiveStatus>
            <Text>{text("ui.analysis.finding_drives", "Finding drives…")}</Text>
          </LiveStatus>
          <Progress
            label={text("ui.analysis.loading_targets", "Loading targets")}
          />
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
      <EmptyDriveNotice targets={targets} />
      {entries.length > 0 && (
        <>
          <ChoiceGroup
            id={driveGroupId}
            label={text("ui.analysis.choose_target_label", "Choose a target")}
            value={selected?.raw ?? null}
            isDisabled={disabled}
            onChange={(raw) =>
              choose(entries.find(({ path }) => path.raw === raw)?.path)
            }
          >
            {listed.map((target) => {
              const probe = targets.probes[target.path.raw];
              const isSelected = target.path.raw === selected?.raw;
              const { access, deferred } = shownAccess(
                target,
                probe,
                targets.finished,
                {
                  selected: isSelected,
                  reviewing: review.reviewing,
                  reviewed: review.targets,
                },
              );
              deniedNetworkVolume ||= deferred && access === "denied";
              const capacity = describeTargetCapacity(
                probe,
                checkingCapacity,
                text,
                regional,
              );
              const kind = target.kind
                ? ({
                    internal: text("ui.analysis.target_internal", "internal"),
                    external: text("ui.analysis.target_external", "external"),
                    network_mount: text(
                      "ui.analysis.target_network_mount",
                      "network mount",
                    ),
                    folder_mount: text(
                      "ui.analysis.target_folder_mount",
                      "folder mount",
                    ),
                  }[target.kind] ?? text("ui.common.unknown", "Unknown"))
                : text(
                    "ui.analysis.target_type_unavailable",
                    "Type unavailable",
                  );
              return (
                <Choice
                  key={target.path.raw}
                  value={target.path.raw}
                  label={target.path.display}
                  icon="drive"
                  description={[
                    kind,
                    capacity.evidence,
                    ...(deferred
                      ? [
                          text(
                            "ui.analysis.target_network_checked_on_select",
                            "Network volumes are checked when you select them.",
                          ),
                        ]
                      : []),
                  ].join(" · ")}
                  summary={showDriveDetails ? undefined : capacity.summary}
                  availablePercent={capacity.availablePercent}
                  secondary={target.defaultVisible === false}
                  status={{
                    text: accessLabel(access, text),
                    tone: access === "accessible" ? "good" : "neutral",
                  }}
                />
              );
            })}
          </ChoiceGroup>
          {deniedNetworkVolume && (
            <NetworkVolumeDenied openSettings={review.openPrivacySettings} />
          )}
          <HiddenVolumesToggle
            hidden={hidden}
            shown={showHidden}
            controls={driveGroupId}
            toggle={() => setShowHidden(!showHidden)}
          />
          {!isInitial && (
            <div className="du-drive-picker__details">
              <Action
                variant="quiet"
                aria-expanded={showDriveDetails}
                aria-controls={driveGroupId}
                onPress={() => setShowDriveDetails(!showDriveDetails)}
              >
                {showDriveDetails
                  ? text("ui.analysis.hide_drive_details", "Hide drive details")
                  : text("ui.analysis.drive_details", "Drive details")}
              </Action>
            </div>
          )}
        </>
      )}
    </section>
  );
}

/** Shows or folds the targets outside the default list (F01 #263). */
function HiddenVolumesToggle({
  hidden,
  shown,
  controls,
  toggle,
}: {
  hidden: number;
  shown: boolean;
  controls: string;
  toggle: () => void;
}) {
  const text = useLocaleText();
  if (hidden === 0 && !shown) return null;
  return (
    <Action
      variant="quiet"
      aria-expanded={shown}
      aria-controls={controls}
      onPress={toggle}
    >
      {shown
        ? text("ui.analysis.hide_system_volumes", "Hide system volumes")
        : text(
            "ui.analysis.hidden_system_volumes",
            "{count} system volumes hidden · Show all",
            { count: String(hidden) },
          )}
    </Action>
  );
}

/** Says how to allow a network volume that macOS refused at selection (F01 #266). */
function NetworkVolumeDenied({ openSettings }: { openSettings: () => void }) {
  const text = useLocaleText();
  return (
    <Notice tone="error">
      {text(
        "ui.analysis.network_volume_denied",
        "macOS didn't allow access to this network volume. Turn on Network Volumes for DiskUnfold in System Settings › Privacy & Security › Files and Folders.",
      )}{" "}
      <Action variant="quiet" onPress={openSettings}>
        {text("ui.startup.notice_open_settings", "Open Settings")}
      </Action>
    </Notice>
  );
}

function EmptyDriveNotice({
  targets,
}: {
  targets: ReturnType<typeof useTargets>;
}) {
  const text = useLocaleText();
  if (
    targets.loading ||
    !targets.list ||
    targets.error ||
    targets.list.unavailableReason ||
    targets.list.targets.length > 0
  )
    return null;
  return (
    <Notice tone="info">
      {text(
        "ui.analysis.drives_empty",
        "No drives were found. You can still enter a folder path.",
      )}
    </Notice>
  );
}
