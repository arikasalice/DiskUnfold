import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
} from "react";
import {
  Action,
  Heading,
  LiveStatus,
  Notice,
  Progress,
  Readout,
  Text,
} from "../../components";
import type {
  AnalysisClient,
  StartupAccessAction,
  StartupAccessState,
  StartupCheckResult,
  StartupGrade,
  StartupStatus,
} from "../../ipc";
import "./startup-status.css";

type StartupLanguage = "en" | "ko";

const messages = {
  en: {
    title: "Startup and privacy",
    introduction:
      "Review startup checks and Full Disk Access for this Mac. Disk analysis is not enabled in this build.",
    loading: "Loading startup checks",
    loadFailed: "Startup status could not be read. Try again.",
    actionFailed: "The permission status could not be updated. Try again.",
    retry: "Try again",
    statusHeading: "Startup status",
    checksHeading: "Checks",
    grade: "Overall status",
    access: "Full Disk Access",
    analysis: "Analysis entry",
    allowed: "Allowed by startup checks",
    blocked: "Blocked by startup checks",
    deniedNotice:
      "Full Disk Access is denied. Open Privacy & Security Settings, add DiskUnfold under Full Disk Access, then check the permission again.",
    analysisBlockedNotice:
      "Startup checks do not allow analysis. Review the listed results for details.",
    unknownNotice:
      "macOS could not confirm Full Disk Access. Unknown access is not treated as a denial. You can open Privacy & Security Settings and check again.",
    confirmedNotice:
      "Full Disk Access is confirmed. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    notApplicableNotice:
      "Full Disk Access does not apply to this operation. This build provides startup checks and permission controls only; disk analysis is not enabled.",
    openSettings: "Open Privacy & Security Settings",
    recheck: "Check permission again",
    settingsOpened:
      "System Settings reported that its privacy page opened. Grant Full Disk Access to DiskUnfold, then check the permission again.",
    settingsNotConfirmed:
      "macOS did not confirm that Settings opened. If needed, open System Settings › Privacy & Security › Full Disk Access, add DiskUnfold, then check again.",
    rechecked: "Permission status refreshed.",
    unknown: "Unknown",
    pass: "Passed",
    limited: "Limited",
    outsideVerified: "Outside verified combinations",
    unverifiable: "Could not verify",
    notApplicable: "Not applicable",
    denied: "Denied",
    confirmed: "Confirmed",
    checkNames: {
      resource_integrity: "Built-in resources",
      webview: "WebView availability",
      data_format_and_open: "Local data and format",
      temporary_files: "Temporary files",
      support_matrix: "Verified environment",
      capability: "Platform capabilities",
      full_disk_access: "Full Disk Access",
    } as Record<string, string>,
  },
  ko: {
    title: "시작 상태와 개인정보 보호",
    introduction:
      "이 Mac의 시작 점검과 전체 디스크 접근 권한을 확인합니다. 이 빌드에서는 디스크 분석을 사용할 수 없습니다.",
    loading: "시작 점검을 불러오는 중",
    loadFailed: "시작 상태를 읽지 못했습니다. 다시 시도하세요.",
    actionFailed: "권한 상태를 갱신하지 못했습니다. 다시 시도하세요.",
    retry: "다시 시도",
    statusHeading: "시작 상태",
    checksHeading: "점검 항목",
    grade: "전체 상태",
    access: "전체 디스크 접근 권한",
    analysis: "분석 진입",
    allowed: "시작 점검에서 허용됨",
    blocked: "시작 점검에서 차단됨",
    deniedNotice:
      "전체 디스크 접근 권한이 거부되었습니다. 개인정보 보호 및 보안 설정에서 DiskUnfold에 전체 디스크 접근 권한을 부여한 뒤 다시 확인하세요.",
    analysisBlockedNotice:
      "시작 점검 결과 분석을 허용하지 않습니다. 자세한 내용은 점검 결과를 확인하세요.",
    unknownNotice:
      "macOS에서 전체 디스크 접근 권한을 확인하지 못했습니다. 확인 불가는 거부로 처리하지 않습니다. 설정을 열어 직접 확인할 수 있습니다.",
    confirmedNotice:
      "전체 디스크 접근 권한을 확인했습니다. 이 빌드는 시작 점검과 권한 제어만 제공하며 디스크 분석은 사용할 수 없습니다.",
    notApplicableNotice:
      "이 작업에는 전체 디스크 접근 권한이 적용되지 않습니다. 이 빌드는 시작 점검과 권한 제어만 제공하며 디스크 분석은 사용할 수 없습니다.",
    openSettings: "개인정보 보호 및 보안 설정 열기",
    recheck: "권한 다시 확인",
    settingsOpened:
      "시스템 설정에서 개인정보 보호 화면을 열었다고 알렸습니다. DiskUnfold에 권한을 부여한 뒤 다시 확인하세요.",
    settingsNotConfirmed:
      "macOS에서 설정 화면 열림을 확인하지 못했습니다. 필요하면 시스템 설정 › 개인정보 보호 및 보안 › 전체 디스크 접근 권한에서 DiskUnfold를 추가한 뒤 다시 확인하세요.",
    rechecked: "권한 상태를 다시 확인했습니다.",
    unknown: "알 수 없음",
    pass: "통과",
    limited: "제한됨",
    outsideVerified: "검증 범위 밖",
    unverifiable: "확인 불가",
    notApplicable: "해당 없음",
    denied: "거부됨",
    confirmed: "확인됨",
    checkNames: {
      resource_integrity: "내장 자원",
      webview: "웹뷰 사용 가능 여부",
      data_format_and_open: "로컬 데이터 및 형식",
      temporary_files: "임시 파일",
      support_matrix: "검증 환경",
      capability: "플랫폼 능력",
      full_disk_access: "전체 디스크 접근 권한",
    } as Record<string, string>,
  },
} satisfies Record<StartupLanguage, Record<string, unknown>>;

/** Selects the startup screen language from the WebView's current locale tags. */
export function startupLanguageFrom(tags: readonly string[]): StartupLanguage {
  return tags[0]?.toLowerCase().split(/[-_]/, 1)[0] === "ko" ? "ko" : "en";
}

function startupLanguage(): StartupLanguage {
  if (typeof navigator === "undefined") return "en";
  return startupLanguageFrom([...navigator.languages, navigator.language]);
}

function gradeLabel(
  grade: StartupGrade,
  copy: (typeof messages)[StartupLanguage],
) {
  switch (grade) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    default:
      return copy.unknown;
  }
}

function accessLabel(
  access: StartupAccessState,
  copy: (typeof messages)[StartupLanguage],
) {
  switch (access) {
    case "confirmed":
      return copy.confirmed;
    case "denied":
      return copy.denied;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

function outcomeLabel(
  result: StartupCheckResult,
  copy: (typeof messages)[StartupLanguage],
) {
  switch (result.result) {
    case "pass":
      return copy.pass;
    case "limited":
      return copy.limited;
    case "outside_verified":
      return copy.outsideVerified;
    case "unverifiable":
      return copy.unverifiable;
    case "not_applicable":
      return copy.notApplicable;
    default:
      return copy.unknown;
  }
}

function checkLabel(
  check: StartupCheckResult,
  copy: (typeof messages)[StartupLanguage],
) {
  return copy.checkNames[check.check] ?? copy.unknown;
}

export function StartupStatusScreen({ client }: { client: AnalysisClient }) {
  const language = useMemo(startupLanguage, []);
  const copy = messages[language];
  const [status, setStatus] = useState<StartupStatus>();
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [actionFailed, setActionFailed] = useState(false);
  const [pending, setPending] = useState<StartupAccessAction>();
  const [feedback, setFeedback] = useState<string>();

  useLayoutEffect(() => {
    const root = document.documentElement;
    const previousLanguage = root.lang;
    root.lang = language;
    return () => {
      root.lang = previousLanguage;
    };
  }, [language]);

  const loadStatus = useCallback(async () => {
    setLoading(true);
    setLoadFailed(false);
    try {
      setStatus(await client.call("app.startup_status", {}));
    } catch {
      setLoadFailed(true);
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const runAccessAction = useCallback(
    async (action: StartupAccessAction) => {
      setPending(action);
      setActionFailed(false);
      setFeedback(undefined);
      try {
        const response = await client.call("app.startup_access", { action });
        if (action === "open_privacy_settings") {
          setFeedback(
            response.settingsOpened
              ? copy.settingsOpened
              : copy.settingsNotConfirmed,
          );
        }
        const refreshed = await client.call("app.startup_status", {});
        setStatus(refreshed);
        if (action === "recheck") setFeedback(copy.rechecked);
      } catch {
        setActionFailed(true);
      } finally {
        setPending(undefined);
      }
    },
    [client, copy],
  );

  if (loading && !status) {
    return (
      <main className="du-app du-startup-screen">
        <Heading level={1}>{copy.title}</Heading>
        <Progress label={copy.loading} />
      </main>
    );
  }

  if (loadFailed && !status) {
    return (
      <main className="du-app du-startup-screen">
        <Heading level={1}>{copy.title}</Heading>
        <Notice tone="error">{copy.loadFailed}</Notice>
        <div className="du-startup__actions">
          <Action variant="primary" onPress={() => void loadStatus()}>
            {copy.retry}
          </Action>
        </div>
      </main>
    );
  }

  if (!status) return null;

  const notice =
    status.access === "denied"
      ? copy.deniedNotice
      : !status.analysisAllowed
        ? copy.analysisBlockedNotice
        : status.access === "confirmed"
          ? copy.confirmedNotice
          : status.access === "not_applicable"
            ? copy.notApplicableNotice
            : copy.unknownNotice;

  return (
    <main className="du-app du-startup-screen">
      <header className="du-startup__header">
        <Heading level={1}>{copy.title}</Heading>
        <Text>{copy.introduction}</Text>
      </header>
      <div className="du-startup__panels">
        <section
          className="du-startup__panel"
          aria-labelledby="du-startup-status"
        >
          <Heading level={2} id="du-startup-status">
            {copy.statusHeading}
          </Heading>
          <Notice tone={!status.analysisAllowed ? "error" : "info"}>
            {notice}
          </Notice>
          <Readout
            label={copy.statusHeading}
            rows={[
              { label: copy.grade, value: gradeLabel(status.grade, copy) },
              { label: copy.access, value: accessLabel(status.access, copy) },
              {
                label: copy.analysis,
                value: status.analysisAllowed ? copy.allowed : copy.blocked,
              },
            ]}
          />
        </section>
        <section
          className="du-startup__panel"
          aria-labelledby="du-startup-checks"
        >
          <Heading level={2} id="du-startup-checks">
            {copy.checksHeading}
          </Heading>
          <Readout
            label={copy.checksHeading}
            rows={status.checks.map((check, index) => ({
              label: checkLabel(check, copy),
              value: outcomeLabel(check, copy),
              wideValue: true,
              emphasized:
                check.check === "full_disk_access" &&
                check.result === "blocked",
              key: `${check.check}-${index}`,
            }))}
          />
        </section>
      </div>
      {actionFailed && <Notice tone="error">{copy.actionFailed}</Notice>}
      {feedback && <LiveStatus>{feedback}</LiveStatus>}
      <div className="du-startup__actions">
        <Action
          variant="primary"
          isPending={pending === "open_privacy_settings"}
          isDisabled={loading || pending !== undefined}
          onPress={() => void runAccessAction("open_privacy_settings")}
        >
          {copy.openSettings}
        </Action>
        <Action
          isPending={pending === "recheck"}
          isDisabled={loading || pending !== undefined}
          onPress={() => void runAccessAction("recheck")}
        >
          {copy.recheck}
        </Action>
      </div>
    </main>
  );
}
