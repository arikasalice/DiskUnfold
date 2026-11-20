import { expect, test } from "vitest";
import {
  amountToBytes,
  conditionFromRows,
  countConditions,
  decodeNameOperand,
  draftFromRule,
  encodeNameOperand,
  newRow,
  rowsFromCondition,
  ruleProblems,
  toRuleDraft,
  trimGroupName,
} from "../src/state/groups-conditions";
import type { ConditionRow, DraftRule } from "../src/state/groups-conditions";
import type { GroupRule, QueryCondition } from "../src/ipc";
import { ARCHIVE, MOVIES, ruleset } from "./groups-fixtures";

test("F36 FR3: a name is encoded as base64url UTF-16LE and decoded back, including non-BMP text", () => {
  expect(encodeNameOperand("mp4")).toEqual({
    encoding: "windows_utf16le",
    value: "bQBwADQA",
  });
  for (const name of ["mp4", "영상 파일", "a_b-c", "\u{1F600}x"]) {
    const { encoding, value } = encodeNameOperand(name);
    expect(decodeNameOperand(value, encoding)).toBe(name);
  }
  expect(decodeNameOperand("not base64!", "windows_utf16le")).toBeUndefined();
  // An odd number of bytes is not UTF-16.
  expect(decodeNameOperand("bQBw", "windows_utf16le")).toBeUndefined();
});

test("#267: on macOS and Linux a name is encoded as UTF-8 bytes like a typed path", () => {
  const host = window as unknown as { webkit?: unknown };
  host.webkit = { messageHandlers: { diskunfold: { postMessage() {} } } };
  try {
    expect(encodeNameOperand("mp4")).toEqual({
      encoding: "bytes",
      value: "bXA0",
    });
    for (const name of ["mp4", "영상 파일", "a_b-c", "\u{1F600}x"]) {
      const { encoding, value } = encodeNameOperand(name);
      expect(encoding).toBe("bytes");
      expect(decodeNameOperand(value, encoding)).toBe(name);
    }
    // A byte that is not UTF-8 is shown as bytes, not guessed.
    expect(decodeNameOperand("_w", "bytes")).toBeUndefined();
    // The same operand reads by its own encoding, whatever the host is now.
    expect(decodeNameOperand("bQBwADQA", "windows_utf16le")).toBe("mp4");
  } finally {
    delete host.webkit;
  }
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
  // A blank value is its own problem, apart from a size that is not whole bytes.
  expect(ruleProblems(base, "iec")).toEqual([{ kind: "empty_value", row: 0 }]);
  expect(
    ruleProblems(
      {
        ...base,
        rows: [
          {
            kind: "size",
            basis: "logical",
            op: "at_least",
            amount: " ",
            unit: 0,
          },
        ],
      },
      "iec",
    ),
  ).toEqual([{ kind: "empty_value", row: 0 }]);
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

test("F36 FR3: a condition tree counts its single tests", () => {
  const leaf = (value: "file" | "directory"): QueryCondition => ({
    kind: "predicate",
    predicate: { kind: "type", value },
  });
  expect(countConditions(leaf("file"))).toBe(1);
  expect(
    countConditions({
      kind: "all",
      children: [
        leaf("file"),
        { kind: "not", child: leaf("directory") },
        { kind: "any", children: [leaf("file"), leaf("directory")] },
      ],
    }),
  ).toBe(4);
});

test("F36 FR3: a group name is sent without surrounding Unicode white space, as the engine stores it", () => {
  for (const [input, sent] of [
    ["  Movies", "Movies"],
    ["Movies  ", "Movies"],
    ["\tMovies\t", "Movies"],
    ["\u3000영상\u3000", "영상"],
    ["\u00a0Movies\u00a0", "Movies"],
    // Rust trims U+0085 and keeps U+FEFF; String.prototype.trim does the opposite.
    ["\u0085Movies\u0085", "Movies"],
    ["\ufeffMovies", "\ufeffMovies"],
    [" 보고서 초안 ", "보고서 초안"],
    ["Report \u3000 draft", "Report \u3000 draft"],
  ] as const) {
    expect(trimGroupName(input)).toBe(sent);
  }
  const draft: DraftRule = {
    key: "k",
    groupId: MOVIES,
    displayName: "\u3000보고서 초안\t",
    rows: [{ kind: "name", op: "contains", text: "mp4" }],
    readOnly: undefined,
    preserved: undefined,
  };
  expect(toRuleDraft(draft, "iec").displayName).toBe("보고서 초안");
  expect(ruleProblems(draft, "iec")).toEqual([]);
  for (const blank of ["", " \t", "\u3000\u00a0", "\u0085"])
    expect(ruleProblems({ ...draft, displayName: blank }, "iec")).toEqual([
      { kind: "empty_name" },
    ]);
});
