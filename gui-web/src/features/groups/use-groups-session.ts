import { useEffect, useState } from "react";
import type {
  AnalysisClient,
  GroupExplanation,
  GroupTotals,
  ItemRef,
} from "../../ipc";
import { GroupsController } from "../../state/groups-controller";
import {
  createExplainRead,
  createTotalsRead,
  staleReporter,
} from "../../state/groups-details";
import type {
  ExplainKey,
  LatestRead,
  TotalsKey,
} from "../../state/groups-details";
import { GroupRuleEditor } from "../../state/groups-editor";
import { GroupMembersReader } from "../../state/groups-members";
import type { SizeSystem } from "../../state/groups-conditions";

/** Everything one open virtual-group screen owns. */
export type GroupsSession = {
  controller: GroupsController;
  members: GroupMembersReader;
  totals: LatestRead<TotalsKey, GroupTotals>;
  explain: LatestRead<ExplainKey, GroupExplanation>;
  editor: GroupRuleEditor;
};

/**
 * Creates the screen's controller and readers when it opens and disposes them
 * when it closes or its target changes. A new target starts a new session.
 */
export function useGroupsSession(
  client: AnalysisClient,
  resultVersion: string,
  scope: ItemRef | undefined,
  system: SizeSystem,
): GroupsSession | undefined {
  const [session, setSession] = useState<GroupsSession>();
  const scopeVersion = scope?.resultVersion;
  const scopeItem = scope?.itemNo;
  useEffect(() => {
    const target = {
      resultVersion,
      scope:
        scopeVersion !== undefined && scopeItem !== undefined
          ? { resultVersion: scopeVersion, itemNo: scopeItem }
          : undefined,
    };
    const controller = new GroupsController(client, target);
    const unreadable = (error: unknown) => controller.reportUnreadable(error);
    const members = new GroupMembersReader(client, unreadable);
    const totals = createTotalsRead(client, staleReporter(unreadable));
    const explain = createExplainRead(client, staleReporter(unreadable));
    const editor = new GroupRuleEditor(client, system, (ruleset) =>
      controller.applySavedRuleset(ruleset),
    );
    controller.start();
    setSession({ controller, members, totals, explain, editor });
    return () => {
      controller.dispose();
      members.dispose();
      totals.dispose();
      explain.dispose();
      editor.dispose();
    };
  }, [client, resultVersion, scopeVersion, scopeItem, system]);
  return session;
}
