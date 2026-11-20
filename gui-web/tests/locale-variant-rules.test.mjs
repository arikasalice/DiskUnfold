import { expect, test } from "vitest";
import { variantDeclarationErrors } from "../scripts/locale-variant-rules.mjs";

test("locale variants require their base key", () => {
  expect(
    variantDeclarationErrors(new Map([["ui.example.unix", "[]"]])),
  ).toEqual(["Locale variant is missing base key: ui.example.unix"]);
});

test("OS-name keys outside UI wording remain standalone", () => {
  expect(
    variantDeclarationErrors(new Map([["cli.support.os.macos", "[]"]])),
  ).toEqual([]);
});

test("locale variants require the same declared parameters as their base", () => {
  expect(
    variantDeclarationErrors(
      new Map([
        ["ui.example", '[["count","count"]]'],
        ["ui.example.macos", '[["count","number"]]'],
      ]),
    ),
  ).toEqual([
    "Locale variant parameters differ from base key: ui.example.macos",
  ]);
});
