import { useEffect, useLayoutEffect, useMemo } from "react";
import type { ReactNode } from "react";
import { Action, Notice, Progress } from "../components";
import { LocaleTextContext, useLocaleText } from "../i18n/presentation";
import { RegionalFormatContext } from "../i18n/regional-format";
import type { RegionalFormat } from "../i18n/regional-format";
import type { AnalysisClient } from "../ipc";
import {
  createLocaleText,
  createVerifiedBundleText,
} from "../state/locale-text";
import { makeLocaleReporter } from "../state/locale-missing";
import { useLocale } from "../state/use-locale";
import type { LocaleView } from "../state/use-locale";
import { App } from "./App";

function regionalFormat({ selected, profile }: LocaleView): RegionalFormat {
  const { decimalSeparator, groupingSeparator, groupingSize, datePatterns } =
    selected.formatTable;
  const byteUnit = selected.messages["ui.unit.byte"];
  return {
    decimalSeparator,
    groupingSeparator,
    groupingSize,
    datePattern:
      datePatterns[profile.dateFormat] ??
      datePatterns.lang_default ??
      "yyyy-MM-dd'T'HH:mm:ss",
    dateFormat: profile.dateFormat,
    groupNumbers: profile.numberFormat === "lang_default",
    timeZonePolicy: profile.timeZonePolicy,
    localTimeZoneStatus: profile.localTimeZoneStatus,
    ...(byteUnit?.kind === "text" && { byteUnitText: byteUnit.template }),
  };
}

function useLocalePresentation(view: LocaleView | undefined) {
  const language = view?.language;
  const scale = view?.profile.appScale;
  const motion = view?.profile.motion;
  useLayoutEffect(() => {
    if (!language || scale === undefined) return;
    const root = document.documentElement;
    const previousLanguage = root.lang;
    root.lang = language;
    root.style.setProperty("--du-app-scale", String(scale / 100));
    const media =
      typeof matchMedia === "function"
        ? matchMedia("(prefers-reduced-motion: reduce)")
        : undefined;
    function setMotion() {
      root.dataset.duMotion =
        motion === "reduce" || media?.matches ? "reduce" : "standard";
    }
    setMotion();
    media?.addEventListener("change", setMotion);
    return () => {
      media?.removeEventListener("change", setMotion);
      delete root.dataset.duMotion;
      root.style.removeProperty("--du-app-scale");
      root.lang = previousLanguage;
    };
  }, [language, motion, scale]);
}

export function DesktopLocale({
  client,
  analysisEnabled = true,
  children,
}: {
  client: AnalysisClient;
  analysisEnabled?: boolean;
  children?: ReactNode;
}) {
  const fallbackText = useLocaleText();
  const { view, startupFallbackBundle, save, retry, errorKey, commandError } =
    useLocale(client, children != null);
  const reporter = useMemo(() => makeLocaleReporter(client), [client]);
  useEffect(() => () => reporter.cancel(), [reporter]);
  const format = useMemo(() => view && regionalFormat(view), [view]);
  const text = useMemo(
    () => view && format && createLocaleText(view, format, reporter.note),
    [view, format, reporter],
  );
  useLocalePresentation(view);
  if (!view || !text) {
    if (children && startupFallbackBundle) {
      const startupText = createVerifiedBundleText(startupFallbackBundle);
      return (
        <LocaleTextContext.Provider value={startupText}>
          <Notice tone="error">{startupText("ui.locale.failed", "")}</Notice>
          <Action onPress={retry}>{startupText("ui.common.retry", "")}</Action>
          {children}
        </LocaleTextContext.Provider>
      );
    }
    return (
      <main className="du-app">
        {errorKey ? (
          <>
            <Notice tone="error">
              {fallbackText(
                "ui.locale.failed",
                "Could not load language settings.",
              )}
            </Notice>
            <Action onPress={retry}>
              {fallbackText("ui.common.retry", "Retry")}
            </Action>
          </>
        ) : (
          <Progress
            label={fallbackText(
              "ui.locale.loading",
              "Loading language settings…",
            )}
          />
        )}
      </main>
    );
  }
  const meta =
    commandError &&
    Object.hasOwn(view.selected.reasonMessages, commandError.reasonKey)
      ? view.selected.reasonMessages[commandError.reasonKey]
      : undefined;
  const issue = commandError && {
    ...commandError,
    cause: text(meta?.causeKey ?? "ui.common.error_generic", ""),
    ...(meta && { action: text(meta.actionKey, "") }),
  };
  return (
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={format}>
        {children ?? (
          <App
            {...(analysisEnabled ? { analysis: client } : {})}
            nativeLocaleHost={!analysisEnabled}
            locale={{
              profile: view.profile,
              text: (key) => text(key, ""),
              save,
              ...(errorKey && { errorKey }),
              ...(issue && { issue }),
            }}
          />
        )}
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
