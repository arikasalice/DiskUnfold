import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
} from "react";
import type { ReactNode } from "react";
import { Action, LiveStatus, Notice, Progress } from "../components";
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
import { applyPresentation } from "../theme/presentation-settings";
import "../theme/locale.css";
import { App } from "./App";
import { SupportRequestContext } from "./support-request";
import { LocaleSettings } from "../features/settings/LocaleSettings";
import type { LocaleSettingsProps } from "../features/settings/LocaleSettings";
import { useLocaleSettingsExtras } from "../features/settings/use-locale-settings-extras";
import { ClosingGuard } from "../features/shutdown/ClosingGuard";
import { StartupGate } from "../features/startup/StartupGate";

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
    // Motion level and scale are decided in one place: the theme module.
    const removePresentation = applyPresentation(root, {
      motion: motion ?? "follow_os",
      appScale: scale,
    });
    return () => {
      removePresentation();
      root.lang = previousLanguage;
    };
  }, [language, motion, scale]);
}

export function DesktopLocale({
  client,
  analysisEnabled = true,
  startupBanner = false,
  children,
}: {
  client: AnalysisClient;
  analysisEnabled?: boolean;
  /** Shows the non-blocking startup banner above the analysis shell. */
  startupBanner?: boolean;
  /**
   * A screen that replaces the app shell (the startup screen where analysis is off). It is shown
   * under the language and display entry, which opens the same settings dialog as the shell header.
   */
  children?: ReactNode;
}) {
  const fallbackText = useLocaleText();
  const { view, startupFallbackBundle, save, retry, errorKey, commandError } =
    useLocale(client, children != null);
  const reporter = useMemo(() => makeLocaleReporter(client), [client]);
  const [supportRequests, setSupportRequests] = useState(0);
  const openSupport = useCallback(
    () => setSupportRequests((count) => count + 1),
    [],
  );
  useEffect(() => () => reporter.cancel(), [reporter]);
  const format = useMemo(() => view && regionalFormat(view), [view]);
  // The size unit is a read-only row, shown only where analysis (and so its settings) is on.
  const settingsExtras = useLocaleSettingsExtras(view, client, analysisEnabled);
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
          <ClosingGuard client={client}>{children}</ClosingGuard>
        </LocaleTextContext.Provider>
      );
    }
    return (
      <ClosingGuard client={client}>
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
      </ClosingGuard>
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
  const locale: LocaleSettingsProps = {
    profile: view.profile,
    text: (key, values) => text(key, "", values),
    save,
    ...(errorKey && { errorKey }),
    ...(issue && { issue }),
    ...settingsExtras,
  };
  const shell = (
    <App
      {...(analysisEnabled ? { analysis: client } : {})}
      nativeLocaleHost={!analysisEnabled}
      infoClient={client}
      locale={locale}
    />
  );
  // A caller's own screen has no shell header, so it gets the same locale entry the header
  // would show (F13 FR3.HD1.gui-web.DD1, FR11, FR12).
  const hosted =
    children != null ? (
      <LocaleEntryScreen locale={locale}>{children}</LocaleEntryScreen>
    ) : undefined;
  return (
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={format}>
        <SupportRequestContext.Provider value={supportRequests}>
          <ClosingGuard client={client}>
            {hosted ??
              (startupBanner ? (
                <StartupGate client={client} onOpenSupport={openSupport}>
                  {shell}
                </StartupGate>
              ) : (
                shell
              ))}
          </ClosingGuard>
        </SupportRequestContext.Provider>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}

/** Puts the language and display entry above a screen that has no shell header. */
function LocaleEntryScreen({
  locale,
  children,
}: {
  locale: LocaleSettingsProps;
  children: ReactNode;
}) {
  return (
    <>
      <header className="du-locale-entry">
        <LocaleSettings {...locale} />
        {locale.errorKey === "ui.locale.failed" && (
          <LiveStatus>{locale.text(locale.errorKey)}</LiveStatus>
        )}
      </header>
      {children}
    </>
  );
}
