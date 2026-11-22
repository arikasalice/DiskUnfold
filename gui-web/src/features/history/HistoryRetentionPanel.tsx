import {
  Action,
  Heading,
  IntegerField,
  LiveStatus,
  Notice,
  Progress,
  StatusBadge,
  SwitchField,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { HistoryRetentionPolicy } from "../../ipc";
import type { RetentionController } from "./useHistoryRetention";
import { HistoryRetentionDialog } from "./HistoryRetentionDialog";
import { parseDays } from "./history-retention-model";
import "../../theme/history-retention.css";

function localizedError(
  key: "count_unavailable" | "change_failed",
  text: ReturnType<typeof useLocaleText>,
) {
  return key === "count_unavailable"
    ? text(
        "ui.history.retention.count_unavailable",
        "History to be removed couldn't be counted, so this can't be changed. Try again shortly.",
      )
    : text(
        "ui.history.retention.change_failed",
        "The retention setting couldn't be changed. Try again.",
      );
}

function RetentionPolicyNotices({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const policyWarning =
    controller.policy?.unavailableReason === "history.policy_unreadable"
      ? text(
          "ui.history.retention.policy_unreadable",
          "Saved retention settings couldn't be read, so automatic cleanup is off.",
        )
      : controller.policy?.unavailableReason === "history.policy_invalid"
        ? text(
            "ui.history.retention.policy_invalid",
            "Saved retention settings aren't valid, so automatic cleanup is off.",
          )
        : undefined;
  const panelError =
    controller.panelError === "load_failed"
      ? text(
          "ui.history.retention.load_failed",
          "Retention settings couldn't be loaded. Try again.",
        )
      : controller.panelError
        ? localizedError(controller.panelError, text)
        : undefined;

  return (
    <>
      {policyWarning && <Notice tone="warning">{policyWarning}</Notice>}
      {panelError && <Notice tone="error">{panelError}</Notice>}
    </>
  );
}

function RetentionPeriodControl({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const { policy } = controller;
  const days = parseDays(controller.draftDays);
  const unchanged =
    days !== undefined &&
    days === (policy?.specifiedDays ?? policy?.suggestedDays);
  const fieldError =
    controller.validationAttempted && days === undefined
      ? text(
          "ui.history.retention.invalid_days",
          "Enter a whole number of 1 or more.",
        )
      : undefined;
  const showSuggested =
    policy?.specifiedDays === undefined &&
    !controller.suggestionChanged &&
    controller.draftDays === String(policy?.suggestedDays);

  return (
    <div className="du-history-retention__period">
      <IntegerField
        label={text("ui.history.retention.keep_for", "Keep history for")}
        value={controller.draftDays}
        unit={text("ui.history.retention.days", "days")}
        after={
          showSuggested ? (
            <StatusBadge>
              {text("ui.history.retention.suggested", "Suggested")}
            </StatusBadge>
          ) : undefined
        }
        error={fieldError}
        inputRef={controller.inputRef}
        isDisabled={controller.loading || controller.busy || !policy}
        onChange={controller.setDraftDays}
      />
      <Action
        isDisabled={
          controller.loading || controller.busy || !policy || unchanged
        }
        onPress={controller.applyPeriod}
      >
        {text("ui.history.retention.apply", "Apply")}
      </Action>
    </div>
  );
}

function RetentionAnnouncement({
  announcement,
}: {
  announcement:
    | {
        key: "enabled" | "disabled" | "period_saved";
        days?: number;
        sequence: number;
      }
    | undefined;
}) {
  const text = useLocaleText();
  if (!announcement) return null;
  let message: string;
  if (announcement.key === "period_saved")
    message = text(
      "ui.history.retention.period_saved",
      "Retention was changed to {days} days.",
      { days: BigInt(announcement.days ?? 1) },
    );
  else if (announcement.key === "enabled")
    message = text(
      "ui.history.retention.enabled",
      "Automatic cleanup was turned on.",
    );
  else
    message = text(
      "ui.history.retention.disabled",
      "Automatic cleanup was turned off.",
    );
  return <LiveStatus key={announcement.sequence}>{message}</LiveStatus>;
}

/** Renders the non-modal history retention side panel and its confirmation. */
export function HistoryRetentionPanel({
  controller,
}: {
  controller: RetentionController;
}) {
  const text = useLocaleText();
  const { policy, activeReview } = controller;

  return (
    <aside
      id="du-history-retention-panel"
      className="du-history-retention"
      aria-labelledby="du-history-retention-title"
      aria-hidden={!controller.isOpen || undefined}
      inert={!controller.isOpen}
      data-open={controller.isOpen || undefined}
    >
      <header className="du-history-retention__header">
        <Heading
          level={2}
          id="du-history-retention-title"
          ref={controller.titleRef}
          programmaticFocus
        >
          {text("ui.history.retention.panel_title", "History retention")}
        </Heading>
        <Action
          variant="quiet"
          icon="close"
          isDisabled={controller.busy}
          aria-label={text(
            "ui.history.retention.close",
            "Close retention settings",
          )}
          onPress={controller.closePanel}
        />
      </header>
      <div className="du-history-retention__body">
        <RetentionPolicyNotices controller={controller} />
        <Text>
          {text(
            "ui.history.retention.description",
            "Only task history is cleaned up. Files on disk and the Recycle Bin don't change.",
          )}
        </Text>
        {controller.loading && (
          <Progress
            label={text(
              "ui.history.retention.loading",
              "Loading retention settings",
            )}
          />
        )}
        <SwitchField
          label={text(
            "ui.history.retention.auto_cleanup",
            "Clean up automatically",
          )}
          isSelected={policy?.enabled ?? false}
          isDisabled={controller.loading || controller.busy || !policy}
          onChange={controller.changeEnabled}
        />
        <RetentionPeriodControl controller={controller} />
        <RetentionAnnouncement announcement={controller.announcement} />
      </div>
      {activeReview && (
        <HistoryRetentionDialog
          activeReview={activeReview}
          busy={controller.busy}
          onConfirm={controller.confirmReview}
          onCancel={controller.cancelActiveReview}
        />
      )}
    </aside>
  );
}

export function retentionHeaderText(
  policy: HistoryRetentionPolicy | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  if (!policy?.enabled) return undefined;
  return text(
    "ui.history.retention.header_line",
    "History older than {days} days is cleaned up automatically.",
    { days: BigInt(policy.specifiedDays ?? policy.suggestedDays) },
  );
}
