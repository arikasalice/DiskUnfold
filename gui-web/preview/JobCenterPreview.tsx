import { useEffect, useRef, useState } from "react";
import { Heading, Text, Toast } from "../src/components";
import { JobCenterEntry } from "../src/features/jobs/JobCenterEntry";
import { JobCenterPresenter } from "../src/features/jobs/JobCenterPresenter";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { regional } from "./BrowseScanStatusPreview";
import { koreanText } from "./result-summary-fixture";
import type { JobCenterGalleryScene } from "./surface-fixtures";
import { jobCenterSceneJobRef, jobCenterSnapshot } from "./surface-fixtures";

/** Product-shaped preview shell for the fixed Job Center scenes. */
export function JobCenterPreview({ scene }: { scene: string }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(true);
  const knownScene: JobCenterGalleryScene = isScene(scene) ? scene : "main";
  const snapshot = jobCenterSnapshot(knownScene);
  const activeCount = snapshot.inProgress.length;
  const showToast = knownScene === "toast-open-panel";
  const [toastVersion, setToastVersion] = useState(0);

  useEffect(() => {
    if (showToast) setToastVersion((version) => version + 1);
  }, [showToast]);

  return (
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <main className="du-app du-job-center-preview">
          <header className="du-app-header">
            <div className="du-brand">
              <Heading level={1}>DiskUnfold</Heading>
            </div>
            <div className="du-app-header__actions">
              <JobCenterEntry
                clientAvailable
                historyOpen={false}
                count={activeCount}
                expanded={open}
                trigger={trigger}
                onPress={() => setOpen((value) => !value)}
              />
            </div>
          </header>
          <section className="du-job-center-preview__workspace">
            <Heading level={2}>분석</Heading>
            <Text kind="muted">
              작업 패널이 열려 있어도 분석 화면을 사용할 수 있습니다.
            </Text>
            <div
              className="du-job-center-preview__placeholder"
              aria-hidden="true"
            />
          </section>
          <div
            className="du-job-center-overlay"
            data-panel-open={open || undefined}
          >
            <JobCenterPresenter
              isOpen={open}
              trigger={trigger}
              snapshot={snapshot}
              onClose={() => setOpen(false)}
              onOpenHistory={() => setOpen(false)}
              onOpenJob={() => setOpen(false)}
              onRetry={() => {}}
              onControl={() => {}}
              {...(knownScene === "cancel-confirm"
                ? { initialCancelJobRef: jobCenterSceneJobRef }
                : {})}
            />
            <Toast
              message={showToast ? "디스크 분석이 끝났습니다" : ""}
              icon="success"
              version={toastVersion}
            />
          </div>
        </main>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}

function isScene(scene: string): scene is JobCenterGalleryScene {
  return [
    "main",
    "request-pending",
    "resync",
    "empty",
    "cancel-confirm",
    "forced-colors",
    "toast-open-panel",
  ].includes(scene);
}
