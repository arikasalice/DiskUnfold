import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, ReactNode, RefObject, SetStateAction } from "react";
import { LiveStatus, Toast } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { AnalysisClient } from "../../ipc";
import { useJobCenter } from "../../state/job-center";
import type { JobCenterCard } from "../../state/job-center";
import { JobCenterPresenter } from "./JobCenterPresenter";

/**
 * Owns the Job Center state and returns its two parts: the header entry and the
 * overlay (panel, announcer and toast). The app shell places the overlay after
 * the screen content so the closed panel never precedes it in document order.
 */
export function useJobCenterParts({
  client,
  isOpen,
  trigger,
  renderEntry,
  onOpenChange,
  onOpenHistory,
  onOpenJob,
}: {
  client: AnalysisClient | undefined;
  isOpen: boolean;
  trigger: RefObject<HTMLButtonElement | null>;
  renderEntry: (count: number, onPress: () => void) => ReactNode;
  onOpenChange: Dispatch<SetStateAction<boolean>>;
  onOpenHistory: () => void;
  onOpenJob: (card: JobCenterCard) => void;
}): { entry: ReactNode; overlay: ReactNode } {
  const text = useLocaleText();
  const restoreFocus = useRef(false);
  const [toast, setToast] = useState<{
    message: string;
    icon: "success" | "error";
    version: number;
  }>({ message: "", icon: "success", version: 0 });
  const [announcement, setAnnouncement] = useState<{
    id: number;
    message: string;
    target: "panel" | "global";
  }>();
  const finishedAnnouncementId = useRef(0);
  const announceFinishedJob = useCallback(
    (card: JobCenterCard) => {
      const notice = finishedJobNotice(card, text);
      const id = ++finishedAnnouncementId.current;
      setAnnouncement({
        id,
        message: notice.message,
        target: isOpen ? "panel" : "global",
      });
      if (!isOpen)
        setToast((previous) => ({
          message: notice.message,
          icon: notice.icon,
          version: previous.version + 1,
        }));
    },
    [isOpen, text],
  );
  const jobCenter = useJobCenter(client, announceFinishedJob);
  const close = useCallback(() => {
    restoreFocus.current = true;
    onOpenChange(false);
  }, [onOpenChange]);

  useEffect(() => {
    if (!isOpen && restoreFocus.current) {
      trigger.current?.focus();
      restoreFocus.current = false;
    }
  }, [isOpen, trigger]);

  const entry = renderEntry(jobCenter.inProgress.length, () => {
    restoreFocus.current = false;
    setAnnouncement(undefined);
    onOpenChange((open) => !open);
  });
  const overlay = (
    <>
      {client && (
        <JobCenterPresenter
          isOpen={isOpen}
          trigger={trigger}
          snapshot={jobCenter}
          onClose={close}
          onOpenHistory={() => {
            restoreFocus.current = false;
            setAnnouncement(undefined);
            onOpenChange(false);
            onOpenHistory();
          }}
          onOpenJob={(card) => {
            restoreFocus.current = false;
            setAnnouncement(undefined);
            onOpenChange(false);
            onOpenJob(card);
          }}
          onRetry={jobCenter.retry}
          onControl={jobCenter.control}
          {...(announcement?.target === "panel"
            ? {
                announcement: {
                  id: announcement.id,
                  message: announcement.message,
                },
              }
            : {})}
        />
      )}
      {announcement?.target === "global" && (
        <div className="du-announcer">
          <LiveStatus key={announcement.id}>{announcement.message}</LiveStatus>
        </div>
      )}
      <Toast
        message={toast.message}
        icon={toast.icon}
        version={toast.version}
      />
    </>
  );
  return { entry, overlay };
}

function finishedJobNotice(
  card: JobCenterCard,
  text: ReturnType<typeof useLocaleText>,
) {
  const name = jobKindLabel(card.row.kind, text);
  switch (card.terminal?.state) {
    case "failed":
      return {
        message: text("ui.job_center.toast_failed", "{name} failed", { name }),
        icon: "error" as const,
      };
    case "cancelled":
      return {
        message: text("ui.job_center.toast_cancelled", "{name} was canceled", {
          name,
        }),
        icon: "success" as const,
      };
    default:
      return {
        message: text("ui.job_center.toast_finished", "{name} finished", {
          name,
        }),
        icon: "success" as const,
      };
  }
}

function jobKindLabel(kind: string, text: ReturnType<typeof useLocaleText>) {
  if (kind === "scan.metadata")
    return text("ui.history.kind_disk_analysis", "Disk analysis");
  if (kind === "duplicate.check")
    return text("ui.history.kind_duplicate_check", "Duplicate check");
  return text("ui.history.kind_unavailable", "Operation type unavailable");
}
