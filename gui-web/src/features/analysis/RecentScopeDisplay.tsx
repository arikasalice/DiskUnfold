import { Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { displayTimestamp } from "../../i18n/regional-format";
import type { RegionalFormat } from "../../i18n/regional-format";
import type { RecentScopeOpenTarget } from "../../ipc";

export function targetStatus(
  target: RecentScopeOpenTarget,
  text: LocaleText,
): string {
  switch (target.state) {
    case "matched":
      return text("ui.analysis.recent_volume_matched", "Volume matched");
    case "moved":
      return text("ui.analysis.recent_location_moved", "Location moved");
    case "disconnected":
      return text(
        "ui.analysis.recent_volume_disconnected",
        "Volume disconnected",
      );
    case "unverifiable":
      return text(
        "ui.analysis.recent_volume_unverifiable",
        "Cannot verify that this is the same volume",
      );
  }
}

export function formatDate(value: string, regional?: RegionalFormat): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const selected = regional && displayTimestamp(date.getTime(), regional);
  if (selected) return selected.text;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

export function PathValue({ value, lossy }: { value: string; lossy: boolean }) {
  const text = useLocaleText();
  return (
    <div className="du-recent-scopes__path">
      <Text kind="path">
        {value || text("ui.analysis.recent_empty_path", "(empty path)")}
      </Text>
      {lossy && (
        <Text kind="muted">
          {text(
            "ui.analysis.recent_path_characters_hidden",
            "Some characters cannot be displayed.",
          )}
        </Text>
      )}
    </div>
  );
}
