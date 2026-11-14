import { StatusBadge, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { GroupLimitation } from "../../ipc";
import { limitationLabel } from "./group-text";

/** The limits of a classification as badges; nothing is drawn when there are none. */
export function GroupLimitations({
  limitations,
}: {
  limitations: readonly GroupLimitation[];
}) {
  const text = useLocaleText();
  if (limitations.length === 0) return null;
  return (
    <section
      className="du-groups-limits"
      aria-label={text(
        "ui.groups.limits_label",
        "Limits of this classification",
      )}
    >
      <Text kind="muted">
        {text("ui.groups.limits_label", "Limits of this classification")}
      </Text>
      <div className="du-groups-badges">
        {limitations.map((limitation) => (
          <StatusBadge key={limitation.key} tone="neutral">
            {limitationLabel(limitation.key, limitation.count, text)}
          </StatusBadge>
        ))}
      </div>
    </section>
  );
}
