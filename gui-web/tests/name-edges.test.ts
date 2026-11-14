import { expect, test } from "vitest";
import { nameEdges, pathNameEdges } from "../src/components";

// Every character with the Unicode White_Space property.
const WHITE_SPACE = [
  " ",
  "\t",
  "\n",
  "\v",
  "\f",
  "\r",
  "\u0085",
  "\u00a0",
  "\u1680",
  ...Array.from({ length: 11 }, (_, index) =>
    String.fromCodePoint(0x2000 + index),
  ),
  "\u2028",
  "\u2029",
  "\u202f",
  "\u205f",
  "\u3000",
];

function joined(name: string) {
  const edges = nameEdges(name);
  return edges.leading + edges.middle + edges.trailing;
}

test("each Unicode white space character at an edge counts once", () => {
  for (const space of WHITE_SPACE) {
    const edges = nameEdges(`${space}보고서${space}${space}`);
    expect(edges.leadingCount, JSON.stringify(space)).toBe(1);
    expect(edges.trailingCount, JSON.stringify(space)).toBe(2);
    expect(edges.middle).toBe("보고서");
  }
});

test("invisible characters that are not white space are not marked", () => {
  for (const other of ["\u200b", "\ufeff", "\u180e", "\u2060"]) {
    const edges = nameEdges(`${other}a${other}`);
    expect(edges.leadingCount + edges.trailingCount).toBe(0);
  }
});

test("white space inside the name stays in the name", () => {
  expect(nameEdges("보고서 초안")).toEqual({
    leading: "",
    middle: "보고서 초안",
    trailing: "",
    leadingCount: 0,
    trailingCount: 0,
  });
  const edges = nameEdges("  2026 보고서\u00a0");
  expect(edges).toMatchObject({
    middle: "2026 보고서",
    leadingCount: 2,
    trailingCount: 1,
  });
});

test("a name of only white space is marked in full and an empty name not at all", () => {
  expect(nameEdges(" \t ")).toEqual({
    leading: " \t ",
    middle: "",
    trailing: "",
    leadingCount: 3,
    trailingCount: 0,
  });
  expect(nameEdges("")).toEqual({
    leading: "",
    middle: "",
    trailing: "",
    leadingCount: 0,
    trailingCount: 0,
  });
});

test("a name ending with a dot has no mark", () => {
  expect(nameEdges("보고서.")).toMatchObject({
    middle: "보고서.",
    leadingCount: 0,
    trailingCount: 0,
  });
});

test("combining marks and emoji stay whole and a space that carries a mark is not blank", () => {
  expect(nameEdges("e\u0301 ")).toMatchObject({
    middle: "e\u0301",
    trailingCount: 1,
  });
  // The space is the base of the combining acute accent, so it is drawn.
  expect(nameEdges(" \u0301초안")).toMatchObject({
    leadingCount: 0,
    middle: " \u0301초안",
  });
  expect(nameEdges("\u{1f468}\u200d\u{1f469}\u200d\u{1f467} ")).toMatchObject({
    middle: "\u{1f468}\u200d\u{1f469}\u200d\u{1f467}",
    trailingCount: 1,
  });
  // A CR LF pair is one grapheme cluster of two white space characters.
  expect(nameEdges("a\r\n").trailingCount).toBe(2);
});

test("right-to-left names keep their marks at the logical start and end", () => {
  expect(nameEdges("\u05e9\u05dc\u05d5\u05dd ")).toMatchObject({
    leadingCount: 0,
    middle: "\u05e9\u05dc\u05d5\u05dd",
    trailingCount: 1,
  });
  expect(nameEdges(" \u0645\u0644\u0641")).toMatchObject({
    leadingCount: 1,
    middle: "\u0645\u0644\u0641",
    trailingCount: 0,
  });
});

test("the parts always join back into the name", () => {
  for (const name of ["", " ", "a", " a b ", "\t\u3000x\u00a0", "e\u0301 "])
    expect(joined(name)).toBe(name);
});

test("a path marks only its last component", () => {
  expect(pathNameEdges("C:\\Users\\ 민서\\보고서 ")).toMatchObject({
    head: "C:\\Users\\ 민서\\",
    name: { middle: "보고서", trailingCount: 1, leadingCount: 0 },
    tail: "",
  });
  expect(pathNameEdges("/home/민서/ 초안/")).toMatchObject({
    head: "/home/민서/",
    name: { middle: "초안", leadingCount: 1 },
    tail: "/",
  });
  expect(pathNameEdges("C:\\")).toMatchObject({
    head: "",
    name: { middle: "C:" },
    tail: "\\",
  });
  expect(pathNameEdges("/")).toMatchObject({ head: "", tail: "/" });
});
