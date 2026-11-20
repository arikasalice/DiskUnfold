import { describe, expect, test } from "vitest";
import { scanIssueReasonText } from "../src/ipc";
import { realLocales } from "./locale-real";

// F02 FR5.HD1 (#233): why an uncertain entry changed reads as the reviewed text.
describe("uncertain entry reasons", () => {
  test.each([
    [
      "scan.item_replaced",
      "Replaced by another item during the scan",
      "스캔 중 다른 항목으로 바뀜",
    ],
    [
      "scan.item_kind_changed",
      "Its type changed during the scan",
      "스캔 중 유형이 바뀜",
    ],
    [
      "scan.item_unverifiable",
      "The item could not be verified",
      "항목 확인 불가",
    ],
  ] as const)("%s", (key, english, korean) => {
    expect(scanIssueReasonText(key, realLocales.en.text)).toBe(english);
    expect(scanIssueReasonText(key, realLocales.ko.text)).toBe(korean);
  });
});
