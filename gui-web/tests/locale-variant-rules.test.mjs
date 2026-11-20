import { expect, test } from "vitest";
import {
  variantBundleErrors,
  variantDeclarationErrors,
} from "../scripts/locale-variant-rules.mjs";

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

test("a variant only one bundle has is an error", () => {
  const declared = new Map([
    ["ui.example", "[]"],
    ["ui.example.unix", "[]"],
  ]);
  expect(
    variantBundleErrors(declared, [
      ["en", ["ui.example", "ui.example.unix"]],
      ["ko", ["ui.example", "ui.example.unix", "ui.example.macos"]],
    ]),
  ).toEqual(["Locale variant is only in the ko bundle: ui.example.macos"]);
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
