import type { AnalysisClient, GroupRuleIssue, GroupRuleset } from "../ipc";
import { describeFailure } from "./groups-controller";
import {
  draftFromRule,
  newRow,
  ruleProblems,
  toRuleDraft,
} from "./groups-conditions";
import type {
  ConditionRow,
  DraftRule,
  RuleProblem,
  SizeSystem,
} from "./groups-conditions";

/** Phases of the rule editor (FR3.HD1.gui-web.DD1). */
export type EditorPhase =
  "viewing" | "editing" | "saving" | "conflict" | "save_failed";

/** Why the save button is not available. */
export type SaveBlocker =
  "unchanged" | "empty_name" | "empty_condition" | "invalid_input" | "conflict";

/** Immutable view model of the editor. */
export type EditorSnapshot = {
  phase: EditorPhase;
  /** The stored rules the draft started from; its revision guards the save. */
  base: GroupRuleset | undefined;
  drafts: readonly DraftRule[];
  selected: string | undefined;
  /** Review failures of the last rejected save, by draft key. */
  issues: ReadonlyMap<string, string>;
  failure: { reasonKey: string | undefined; retryable: boolean } | undefined;
  /** A change would be lost: the discard confirmation is open. */
  confirmingDiscard: boolean;
  dirty: boolean;
  blocker: SaveBlocker | undefined;
};

function serialize(drafts: readonly DraftRule[], system: SizeSystem): string {
  return JSON.stringify(drafts.map((rule) => toRuleDraft(rule, system)));
}

function blockerOf(
  drafts: readonly DraftRule[],
  dirty: boolean,
  system: SizeSystem,
): SaveBlocker | undefined {
  const problems: RuleProblem[] = drafts.flatMap((rule) =>
    ruleProblems(rule, system),
  );
  if (problems.some((problem) => problem.kind === "empty_name"))
    return "empty_name";
  if (problems.some((problem) => problem.kind === "empty_condition"))
    return "empty_condition";
  if (problems.length > 0) return "invalid_input";
  return dirty ? undefined : "unchanged";
}

/**
 * Edits a copy of the stored rules. Nothing is sent until the user confirms;
 * then the whole list is saved against the revision the draft started from.
 */
export class GroupRuleEditor {
  private state: EditorSnapshot;
  private baseline = "[]";
  private counter = 0;
  private token = 0;
  private readonly listeners = new Set<() => void>();
  private disposed = false;

  constructor(
    private readonly client: AnalysisClient,
    private readonly system: SizeSystem,
    /** Receives the stored ruleset after a successful save or a reload. */
    private readonly onRuleset: (ruleset: GroupRuleset) => void,
  ) {
    this.state = this.snapshot("viewing", undefined, [], undefined);
  }

  getSnapshot = (): EditorSnapshot => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  dispose(): void {
    this.disposed = true;
    this.token += 1;
    this.listeners.clear();
  }

  /** Opens the editor on a copy of the stored rules. */
  open(ruleset: GroupRuleset): void {
    if (this.state.phase !== "viewing") return;
    const drafts = ruleset.rules.map((rule) =>
      draftFromRule(rule, this.system, this.nextKey()),
    );
    this.baseline = serialize(drafts, this.system);
    this.state = this.snapshot("editing", ruleset, drafts, drafts[0]?.key);
    this.emit();
  }

  /** Closes the editor, or asks before discarding changes. */
  requestClose(): void {
    const { phase, dirty } = this.state;
    if (phase === "saving" || phase === "viewing") return;
    if (dirty) this.update({ confirmingDiscard: true });
    else this.close();
  }

  /** The user confirmed discarding the draft. */
  discard(): void {
    if (this.state.phase !== "saving") this.close();
  }

  /** The user chose to keep editing instead of discarding. */
  keepEditing(): void {
    this.update({ confirmingDiscard: false });
  }

  select(key: string): void {
    if (this.editable()) this.update({ selected: key });
  }

  addRule(): void {
    if (!this.editable()) return;
    const rule: DraftRule = {
      key: this.nextKey(),
      groupId: undefined,
      displayName: "",
      rows: [newRow("name")],
      readOnly: undefined,
      preserved: undefined,
    };
    this.change([...this.state.drafts, rule], rule.key);
  }

  removeRule(key: string): void {
    if (!this.editable()) return;
    const drafts = this.state.drafts.filter((rule) => rule.key !== key);
    const selected =
      this.state.selected === key ? drafts[0]?.key : this.state.selected;
    this.change(drafts, selected);
  }

  moveRule(key: string, offset: -1 | 1): void {
    if (!this.editable()) return;
    const drafts = [...this.state.drafts];
    const from = drafts.findIndex((rule) => rule.key === key);
    const to = from + offset;
    const moved = drafts[from];
    const other = drafts[to];
    if (!moved || !other) return;
    drafts[from] = other;
    drafts[to] = moved;
    this.change(drafts, this.state.selected);
  }

  rename(key: string, displayName: string): void {
    this.edit(key, (rule) => ({ ...rule, displayName }));
  }

  addRow(key: string, kind: ConditionRow["kind"]): void {
    this.edit(key, (rule) =>
      rule.readOnly ? rule : { ...rule, rows: [...rule.rows, newRow(kind)] },
    );
  }

  setRow(key: string, index: number, row: ConditionRow): void {
    this.edit(key, (rule) =>
      rule.readOnly
        ? rule
        : {
            ...rule,
            rows: rule.rows.map((old, i) => (i === index ? row : old)),
          },
    );
  }

  removeRow(key: string, index: number): void {
    this.edit(key, (rule) =>
      rule.readOnly
        ? rule
        : { ...rule, rows: rule.rows.filter((_, i) => i !== index) },
    );
  }

  /** Saves the whole list. Does nothing while blocked or already saving. */
  async save(): Promise<void> {
    const { phase, base, drafts, blocker } = this.state;
    if (!base || blocker !== undefined) return;
    if (phase !== "editing" && phase !== "save_failed") return;
    const token = ++this.token;
    this.update({ phase: "saving", failure: undefined, issues: new Map() });
    const sent = drafts;
    try {
      const response = await this.client.call("group.rules.save", {
        expectedRevision: base.revision,
        rules: sent.map((rule) => toRuleDraft(rule, this.system)),
      });
      if (this.disposed || token !== this.token) return;
      if (response.outcome === "saved") {
        this.onRuleset(response.ruleset);
        this.close();
      } else {
        const issues = issueMap(sent, response.issues);
        this.update({
          phase: "editing",
          issues,
          selected: issues.keys().next().value ?? this.state.selected,
          failure: { reasonKey: response.reasonKey, retryable: false },
        });
      }
    } catch (error) {
      if (this.disposed || token !== this.token) return;
      const failure = describeFailure(error);
      this.update({
        phase:
          failure.reasonKey === "group.rules_modified"
            ? "conflict"
            : "save_failed",
        failure,
      });
    }
  }

  /** After a conflict: replaces the draft with the latest stored rules. */
  async reloadLatest(): Promise<void> {
    const latest = await this.readLatest();
    if (!latest) return;
    const drafts = latest.rules.map((rule) =>
      draftFromRule(rule, this.system, this.nextKey()),
    );
    this.baseline = serialize(drafts, this.system);
    this.onRuleset(latest);
    this.state = this.snapshot("editing", latest, drafts, drafts[0]?.key);
    this.emit();
  }

  /** After a conflict: keeps the draft and guards the next save with the latest revision. */
  async keepMine(): Promise<void> {
    const latest = await this.readLatest();
    if (!latest) return;
    this.onRuleset(latest);
    this.state = {
      ...this.snapshot(
        "editing",
        latest,
        this.state.drafts,
        this.state.selected,
      ),
    };
    this.emit();
  }

  private async readLatest(): Promise<GroupRuleset | undefined> {
    if (this.state.phase !== "conflict") return undefined;
    const token = ++this.token;
    try {
      const latest = await this.client.call("group.rules.get", {});
      return this.disposed || token !== this.token ? undefined : latest;
    } catch (error) {
      if (!this.disposed && token === this.token)
        this.update({ failure: describeFailure(error) });
      return undefined;
    }
  }

  private editable(): boolean {
    const { phase } = this.state;
    return phase === "editing" || phase === "save_failed";
  }

  private nextKey(): string {
    this.counter += 1;
    return `rule-${this.counter}`;
  }

  private edit(key: string, change: (rule: DraftRule) => DraftRule) {
    if (!this.editable()) return;
    const issues = new Map(this.state.issues);
    issues.delete(key);
    this.change(
      this.state.drafts.map((rule) => (rule.key === key ? change(rule) : rule)),
      this.state.selected,
      issues,
    );
  }

  private change(
    drafts: readonly DraftRule[],
    selected: string | undefined,
    issues: ReadonlyMap<string, string> = this.state.issues,
  ) {
    this.state = {
      ...this.state,
      ...this.derived(drafts),
      drafts,
      selected,
      issues,
    };
    this.emit();
  }

  private derived(drafts: readonly DraftRule[]) {
    const dirty = serialize(drafts, this.system) !== this.baseline;
    return { dirty, blocker: blockerOf(drafts, dirty, this.system) };
  }

  private snapshot(
    phase: EditorPhase,
    base: GroupRuleset | undefined,
    drafts: readonly DraftRule[],
    selected: string | undefined,
  ): EditorSnapshot {
    return {
      phase,
      base,
      drafts,
      selected,
      issues: new Map(),
      failure: undefined,
      confirmingDiscard: false,
      ...this.derived(drafts),
    };
  }

  private close() {
    this.token += 1;
    this.baseline = "[]";
    this.state = this.snapshot("viewing", undefined, [], undefined);
    this.emit();
  }

  private update(patch: Partial<EditorSnapshot>) {
    this.state = { ...this.state, ...patch };
    this.emit();
  }

  private emit() {
    for (const listener of [...this.listeners]) listener();
  }
}

function issueMap(
  drafts: readonly DraftRule[],
  issues: readonly GroupRuleIssue[],
): ReadonlyMap<string, string> {
  const map = new Map<string, string>();
  for (const issue of issues) {
    const rule = drafts[issue.index];
    if (rule && !map.has(rule.key)) map.set(rule.key, issue.reasonKey);
  }
  return map;
}
