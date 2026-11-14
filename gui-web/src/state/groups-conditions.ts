import { SIZE_UNIT_RULES, windowsPath } from "../ipc";
import type {
  GroupRule,
  GroupRuleDraft,
  Id,
  QueryCondition,
  QueryItemType,
  QueryNameOp,
  QueryPredicate,
  QuerySizeBasis,
  QuerySizeOp,
} from "../ipc";

/** Size system whose units the size editor offers. */
export type SizeSystem = "iec" | "si";

/** An editable name condition (equal, contains, starts with, ends with). */
export type NameRow = { kind: "name"; op: QueryNameOp; text: string };
/** An editable item-type condition. */
export type TypeRow = { kind: "type"; value: QueryItemType };
/** An editable size condition; `unit` indexes the unit table of the size system. */
export type SizeRow = {
  kind: "size";
  basis: QuerySizeBasis;
  op: QuerySizeOp;
  amount: string;
  unit: number;
};
/** One editable condition; all rows of a rule must match. */
export type ConditionRow = NameRow | TypeRow | SizeRow;

/** Why a rule cannot be edited here: it is kept exactly as it was stored. */
export type ReadOnlyReason = "unregistered" | "unsupported";

/** A rule in the screen's draft. */
export type DraftRule = {
  /** Local identity that survives renames and reordering. */
  key: string;
  groupId: Id | undefined;
  displayName: string;
  rows: ConditionRow[];
  /** Set when the stored condition is kept verbatim and not editable. */
  readOnly: ReadOnlyReason | undefined;
  /** The stored condition of a read-only rule. */
  preserved: QueryCondition | undefined;
};

/** A reason a rule cannot be saved yet. */
export type RuleProblem =
  | { kind: "empty_name" }
  | { kind: "empty_condition" }
  | { kind: "invalid_name"; row: number }
  | { kind: "invalid_size"; row: number };

const U64_MAX = 18_446_744_073_709_551_615n;

/** Encodes a name operand with the Windows UTF-16LE path encoding. */
export function encodeNameOperand(name: string): string {
  return windowsPath(name).raw;
}

/** Decodes a UTF-16LE name operand; undefined when it is not valid text. */
export function decodeNameOperand(raw: string): string | undefined {
  try {
    const binary = atob(raw.replaceAll("-", "+").replaceAll("_", "/"));
    if (binary.length % 2 !== 0) return undefined;
    let text = "";
    for (let index = 0; index < binary.length; index += 2)
      text += String.fromCharCode(
        binary.charCodeAt(index) | (binary.charCodeAt(index + 1) << 8),
      );
    return encodeNameOperand(text) === raw.replace(/=+$/, "")
      ? text
      : undefined;
  } catch {
    return undefined;
  }
}

/** The unit symbols of a size system, with the byte multiplier of each. */
export function sizeUnits(system: SizeSystem) {
  const table = SIZE_UNIT_RULES[system];
  return table.symbols.map((symbol, index) => ({
    symbol,
    multiplier: BigInt(table.base) ** BigInt(index),
  }));
}

/** Parses a decimal amount in a unit into exact bytes; undefined if not whole bytes. */
export function amountToBytes(
  amount: string,
  system: SizeSystem,
  unit: number,
): bigint | undefined {
  const multiplier = sizeUnits(system)[unit]?.multiplier;
  const parts = /^(\d{1,20})(?:\.(\d{1,12}))?$/.exec(amount.trim());
  if (multiplier === undefined || !parts?.[1]) return undefined;
  const fraction = parts[2] ?? "";
  const scale = 10n ** BigInt(fraction.length);
  const scaled = BigInt(parts[1] + fraction) * multiplier;
  if (scaled % scale !== 0n) return undefined;
  const bytes = scaled / scale;
  return bytes <= U64_MAX ? bytes : undefined;
}

function bytesToAmount(bytes: bigint, system: SizeSystem) {
  const units = sizeUnits(system);
  let unit = 0;
  if (bytes > 0n)
    for (let index = units.length - 1; index > 0; index -= 1)
      if (bytes % units[index]!.multiplier === 0n) {
        unit = index;
        break;
      }
  return { amount: (bytes / units[unit]!.multiplier).toString(), unit };
}

function rowFromPredicate(
  predicate: QueryPredicate,
  system: SizeSystem,
): ConditionRow | undefined {
  if (predicate.kind === "type")
    return { kind: "type", value: predicate.value };
  if (predicate.kind === "name") {
    const text =
      predicate.encoding === "windows_utf16le"
        ? decodeNameOperand(predicate.value)
        : undefined;
    return text === undefined
      ? undefined
      : { kind: "name", op: predicate.op, text };
  }
  if (predicate.kind === "size" && /^\d{1,20}$/.test(predicate.bytes)) {
    const bytes = BigInt(predicate.bytes);
    if (bytes > U64_MAX) return undefined;
    return {
      kind: "size",
      basis: predicate.basis,
      op: predicate.op,
      ...bytesToAmount(bytes, system),
    };
  }
  return undefined;
}

/** True when a condition tree uses a predicate the engine does not register. */
export function usesRegisteredPredicate(condition: QueryCondition): boolean {
  if (condition.kind === "predicate")
    return condition.predicate.kind === "registered";
  if (condition.kind === "not") return usesRegisteredPredicate(condition.child);
  return condition.children.some(usesRegisteredPredicate);
}

/** Splits a stored condition into editable rows when it is a plain AND of baseline predicates. */
export function rowsFromCondition(
  condition: QueryCondition,
  system: SizeSystem,
): ConditionRow[] | undefined {
  const leaves =
    condition.kind === "all"
      ? condition.children
      : condition.kind === "predicate"
        ? [condition]
        : undefined;
  if (!leaves) return undefined;
  const rows: ConditionRow[] = [];
  for (const leaf of leaves) {
    if (leaf.kind !== "predicate") return undefined;
    const row = rowFromPredicate(leaf.predicate, system);
    if (!row) return undefined;
    rows.push(row);
  }
  return rows;
}

/** Builds the draft of a stored rule; unsupported or unregistered conditions stay read-only. */
export function draftFromRule(
  rule: GroupRule,
  system: SizeSystem,
  key: string,
): DraftRule {
  const unregistered =
    rule.review === "predicate_unregistered" ||
    usesRegisteredPredicate(rule.condition);
  const rows = unregistered
    ? undefined
    : rowsFromCondition(rule.condition, system);
  const base = { key, groupId: rule.groupId, displayName: rule.displayName };
  if (rows) return { ...base, rows, readOnly: undefined, preserved: undefined };
  return {
    ...base,
    rows: [],
    readOnly: unregistered ? "unregistered" : "unsupported",
    preserved: rule.condition,
  };
}

function predicateFromRow(
  row: ConditionRow,
  system: SizeSystem,
): QueryPredicate {
  if (row.kind === "type") return { kind: "type", value: row.value };
  if (row.kind === "name")
    return {
      kind: "name",
      op: row.op,
      encoding: "windows_utf16le",
      value: encodeNameOperand(row.text),
    };
  return {
    kind: "size",
    basis: row.basis,
    op: row.op,
    bytes: (amountToBytes(row.amount, system, row.unit) ?? 0n).toString(),
  };
}

/** Builds the QueryCondition of editable rows: one predicate, or all of several. */
export function conditionFromRows(
  rows: readonly ConditionRow[],
  system: SizeSystem,
): QueryCondition {
  const leaves = rows.map((row): QueryCondition => ({
    kind: "predicate",
    predicate: predicateFromRow(row, system),
  }));
  return leaves.length === 1 && leaves[0]
    ? leaves[0]
    : { kind: "all", children: leaves };
}

/** The rule as sent to group.rules.save. A read-only rule keeps its stored condition. */
export function toRuleDraft(
  rule: DraftRule,
  system: SizeSystem,
): GroupRuleDraft {
  return {
    ...(rule.groupId !== undefined ? { groupId: rule.groupId } : {}),
    displayName: rule.displayName,
    condition:
      rule.readOnly && rule.preserved
        ? rule.preserved
        : conditionFromRows(rule.rows, system),
  };
}

/** Everything that keeps a rule from being saved. Read-only conditions are never edited. */
export function ruleProblems(
  rule: DraftRule,
  system: SizeSystem,
): RuleProblem[] {
  const problems: RuleProblem[] = [];
  if (rule.displayName.trim() === "") problems.push({ kind: "empty_name" });
  if (rule.readOnly) return problems;
  if (rule.rows.length === 0) problems.push({ kind: "empty_condition" });
  rule.rows.forEach((row, index) => {
    if (row.kind === "name" && row.text.trim() === "")
      problems.push({ kind: "invalid_name", row: index });
    if (
      row.kind === "size" &&
      amountToBytes(row.amount, system, row.unit) === undefined
    )
      problems.push({ kind: "invalid_size", row: index });
  });
  return problems;
}

/** A new empty condition row of a kind. */
export function newRow(kind: ConditionRow["kind"]): ConditionRow {
  if (kind === "type") return { kind, value: "file" };
  if (kind === "name") return { kind, op: "contains", text: "" };
  return { kind, basis: "logical", op: "at_least", amount: "", unit: 0 };
}
