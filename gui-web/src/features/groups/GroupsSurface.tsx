import type { SizeDisplay } from "../../i18n/bytes";
import type { AnalysisClient } from "../../ipc";
import { GroupsView } from "./GroupsView";
import type { GroupsEntry } from "./use-groups-entry";

/** Shows the virtual-group screen while the app shell has it open. */
export function GroupsSurface({
  client,
  browseVersion,
  entry,
  display,
}: {
  client: AnalysisClient | undefined;
  browseVersion: string | undefined;
  entry: GroupsEntry;
  display: SizeDisplay;
}) {
  if (!client || !browseVersion || !entry.open) return null;
  return (
    <GroupsView
      client={client}
      resultVersion={browseVersion}
      scope={entry.scope}
      display={display}
      onClose={entry.close}
    />
  );
}
