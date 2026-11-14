import { useState } from "react";
import { LiveStatus } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type { SampleSession } from "../../ipc";
import { unavailableText } from "./sample-text";

type Kind = "preparing" | "resetting" | "ready" | "reset_done" | "unavailable";
type Heard = { key: string; kind: Kind | undefined; resetOfActive: boolean };

function kindOf(
  session: SampleSession | undefined,
  reached: boolean,
  before: Heard,
): Pick<Heard, "kind" | "resetOfActive"> {
  switch (session?.state) {
    case "preparing":
      return {
        kind: reached ? "resetting" : "preparing",
        resetOfActive: reached,
      };
    case "resetting":
      return { kind: "resetting", resetOfActive: reached };
    case "active":
      return {
        kind:
          before.kind === "resetting" && before.resetOfActive
            ? "reset_done"
            : "ready",
        resetOfActive: false,
      };
    case "unavailable":
      return { kind: "unavailable", resetOfActive: false };
    default:
      return { kind: undefined, resetOfActive: false };
  }
}

function messageOf(
  kind: Kind | undefined,
  session: SampleSession | undefined,
  text: LocaleText,
): string {
  switch (kind) {
    case "preparing":
      return text("ui.sample.preparing", "Preparing the sample data");
    case "resetting":
      return text("ui.sample.resetting", "Resetting the sample experience");
    case "ready":
      return text("ui.sample.announce_ready", "Sample experience is ready");
    case "reset_done":
      return text("ui.sample.announce_reset_done", "Reset complete");
    case "unavailable":
      return text(
        "ui.sample.announce_unavailable",
        "Sample experience unavailable. {reason}",
        { reason: unavailableText(session?.unavailableReason, text) },
      );
    default:
      return "";
  }
}

/**
 * A persistent, visually hidden live region for the sample experience. It
 * speaks once for each state change of the held session: preparing, ready,
 * resetting, reset complete and unavailable. It stays mounted while the
 * visible state moves between the entry button and the sample screen.
 */
export function SampleAnnouncer({
  session,
  reached,
}: {
  session: SampleSession | undefined;
  /** True once the session has been active and has not failed since. */
  reached: boolean;
}) {
  const text = useLocaleText();
  const key = session ? `${session.sessionRef}:${session.state}` : "";
  const [heard, setHeard] = useState<Heard>({
    key: "",
    kind: undefined,
    resetOfActive: false,
  });
  if (heard.key !== key) setHeard({ key, ...kindOf(session, reached, heard) });
  return (
    <div className="du-sample__announcer">
      <LiveStatus>{messageOf(heard.kind, session, text)}</LiveStatus>
    </div>
  );
}
