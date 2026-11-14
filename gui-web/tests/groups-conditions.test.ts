import { expect, test } from "vitest";
import {
  amountToBytes,
  conditionFromRows,
  decodeNameOperand,
  draftFromRule,
  encodeNameOperand,
  newRow,
  rowsFromCondition,
  ruleProblems,
  toRuleDraft,
} from "../src/state/groups-conditions";
import type { ConditionRow, DraftRule } from "../src/state/groups-conditions";
import type { GroupRule, QueryCondition } from "../src/ipc";
import { ARCHIVE, MOVIES, ruleset } from "./groups-fixtures";

test("F36 FR3: a name is encoded as base64url UTF-16LE and decoded back, including non-BMP text", () => {
  expect(encodeNameOperand("mp4")).toBe("bQBwADQA");
  for (const name of ["mp4", "영상 파일", "a_b-c", "\u{1F600}x"]) {
    expect(decodeNameOperand(encodeNameOperand(name))).toBe(name);
  }
  expect(decodeNameOperand("not base64!")).toBeUndefined();
  // An odd number of bytes is not UTF-16.
  expect(decodeNameOperand("bQBw")).toBeUndefined();
});

test("F36 FR3: a size amount must be a whole number of bytes after its unit", () => {
  expect(amountToBytes("10", "iec", 2)).toBe(10_485_760n);
  expect(amountToBytes("1.5", "iec", 1)).toBe(1536n);
  expect(amountToBytes("1.5", "si", 1)).toBe(1500n);
  expect(amountToBytes("0", "iec", 0)).toBe(0n);
  expect(amountToBytes("0.5", "iec", 0)).toBeUndefined();
  expect(amountToBytes("", "iec", 0)).toBeUndefined();
  expect(amountToBytes("-1", "iec", 0)).toBeUndefined();
  expect(amountToBytes("1e3", "iec", 0)).toBeUndefined();
  expect(amountToBytes("18446744073709551615", "iec", 0)).toBe(
    18_446_744_073_709_551_615n,
  );
  expect(amountToBytes("18446744073709551616", "iec", 0)).toBeUndefined();
  expect(amountToBytes("1", "iec", 99)).toBeUndefined();
});

const sizeRows: ConditionRow[] = [
  { kind: "name", op: "ends_with", text: ".mp4" },
  { kind: "type", value: "file" },
  {
    kind: "size",
    basis: "allocated",
    op: "greater_than",
    amount: "2",
    unit: 3,
  },
];

test("F36 FR3: editable rows become a QueryCondition and parse back to the same rows", () => {
  const condition = conditionFromRows(sizeRows, "iec");
  expect(condition).toMatchObject({ kind: "all" });
  const bytes = (2n * 1024n ** 3n).toString();
  expect(JSON.stringify(condition)).toContain(bytes);
  expect(rowsFromCondition(condition, "iec")).toEqual(sizeRows);
  // One row is a single predicate, not an AND of one.
  expect(conditionFromRows([newRow("type")], "iec")).toEqual({
    kind: "predicate",
    predicate: { kind: "type", value: "file" },
  });
});

function ruleWith(
  condition: QueryCondition,
  review: GroupRule["review"] = "ok",
): GroupRule {
  return { ...ruleset().rules[0]!, condition, review };
}

test("F36 FR3: a registered predicate is kept as it was stored and marked as not decidable", () => {
  const registered: QueryCondition = {
    kind: "all",
    children: [
      { kind: "predicate", predicate: { kind: "type", value: "file" } },
      {
        kind: "predicate",
        predicate: {
          kind: "registered",
          id: "role.video",
          revision: 2,
          operand: "AQ",
        },
      },
    ],
  };
  const draft = draftFromRule(ruleWith(registered), "iec", "k1");
  expect(draft.readOnly).toBe("unregistered");
  expect(draft.preserved).toBe(registered);
  expect(toRuleDraft(draft, "iec")).toEqual({
    groupId: MOVIES,
    displayName: "Movies",
    condition: registered,
  });
  // Edits to the other fields never touch the preserved condition.
  expect(ruleProblems({ ...draft, displayName: "" }, "iec")).toEqual([
    { kind: "empty_name" },
  ]);
});

test("F36 FR3: a rule the registry reports as unregistered is read-only even if its shape is plain", () => {
  const plain = ruleset().rules[1]!.condition;
  const draft = draftFromRule(
    ruleWith(plain, "predicate_unregistered"),
    "iec",
    "k2",
  );
  expect(draft.readOnly).toBe("unregistered");
  expect(
    draftFromRule(ruleWith(plain, "converted"), "iec", "k3").readOnly,
  ).toBeUndefined();
});

test("F36 FR3: shapes the editor cannot express stay read-only and unchanged", () => {
  const either: QueryCondition = {
    kind: "any",
    children: [
      { kind: "predicate", predicate: { kind: "type", value: "file" } },
      { kind: "predicate", predicate: { kind: "type", value: "link" } },
    ],
  };
  const byteName: QueryCondition = {
    kind: "predicate",
    predicate: { kind: "name", op: "equal", encoding: "bytes", value: "YQ" },
  };
  for (const condition of [either, byteName]) {
    const draft = draftFromRule(ruleWith(condition), "iec", "k");
    expect(draft.readOnly).toBe("unsupported");
    expect(toRuleDraft(draft, "iec").condition).toBe(condition);
  }
});

test("F36 FR3: every reason a rule cannot be saved is reported", () => {
  const base: DraftRule = {
    key: "k",
    groupId: ARCHIVE,
    displayName: "Archive",
    rows: [newRow("name")],
    readOnly: undefined,
    preserved: undefined,
  };
  expect(ruleProblems({ ...base, displayName: "  " }, "iec")).toContainEqual({
    kind: "empty_name",
  });
  expect(ruleProblems({ ...base, rows: [] }, "iec")).toEqual([
    { kind: "empty_condition" },
  ]);
  expect(ruleProblems(base, "iec")).toEqual([{ kind: "invalid_name", row: 0 }]);
  expect(
    ruleProblems(
      {
        ...base,
        rows: [
          {
            kind: "size",
            basis: "logical",
            op: "at_least",
            amount: "1.5",
            unit: 0,
          },
        ],
      },
      "iec",
    ),
  ).toEqual([{ kind: "invalid_size", row: 0 }]);
  expect(
    ruleProblems(
      { ...base, rows: [{ kind: "name", op: "equal", text: "a" }] },
      "iec",
    ),
  ).toEqual([]);
});
