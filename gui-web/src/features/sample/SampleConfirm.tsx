import { useRef, useState } from "react";
import { ConfirmDialog, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";

export type SampleConfirmKind = "end" | "reset" | "real";

function copy(kind: SampleConfirmKind, text: LocaleText) {
  switch (kind) {
    case "end":
      return {
        title: text(
          "ui.sample.confirm_end_title",
          "End the sample experience?",
        ),
        action: text("ui.sample.end", "End experience"),
      };
    case "reset":
      return {
        title: text(
          "ui.sample.confirm_reset_title",
          "Reset the sample experience?",
        ),
        action: text("ui.sample.reset", "Reset experience"),
      };
    default:
      return {
        title: text(
          "ui.sample.confirm_real_title",
          "Leave the sample and start real analysis?",
        ),
        action: text("ui.sample.real_analysis", "Start real analysis"),
      };
  }
}

/**
 * Asks before a sample command that discards sample state. The dialog keeps
 * both buttons disabled until `run` settles, then returns the focus to the
 * control that asked, when that control still exists.
 */
export function useSampleConfirm(
  run: (kind: SampleConfirmKind) => Promise<void>,
) {
  const text = useLocaleText();
  const [asked, setAsked] = useState<SampleConfirmKind>();
  const [working, setWorking] = useState(false);
  const opener = useRef<Element | null>(null);

  const finish = () => {
    setAsked(undefined);
    const target = opener.current;
    opener.current = null;
    if (target instanceof HTMLElement)
      setTimeout(() => {
        if (target.isConnected) target.focus();
      }, 0);
  };

  const dialog = asked && (
    <ConfirmDialog
      title={copy(asked, text).title}
      confirmLabel={copy(asked, text).action}
      cancelLabel={text("ui.sample.cancel", "Cancel")}
      isPending={working}
      onCancel={finish}
      onConfirm={() => {
        setWorking(true);
        void run(asked).finally(() => {
          setWorking(false);
          finish();
        });
      }}
    >
      <Text>
        {text(
          "ui.sample.confirm_body",
          "Only the sample experience state is discarded. Your real analysis results, plans and settings stay as they are.",
        )}
      </Text>
    </ConfirmDialog>
  );

  return {
    /** `trigger` is the pressed control, which receives the focus back. */
    ask: (kind: SampleConfirmKind, trigger: Element) => {
      opener.current = trigger;
      setAsked(kind);
    },
    dialog,
  };
}
