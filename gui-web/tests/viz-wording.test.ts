import { describe, expect, test } from "vitest";
import { visualizationMessages } from "../src/viz";
import { realLocales } from "./locale-real";

const prefix = "ui.visualization.";
const shown = (language: "en" | "ko") =>
  [...realLocales[language].keys]
    .filter((key) => key.startsWith(prefix))
    .map((key) => {
      const message = realLocales[language].body.messages[key]!;
      const text =
        message.kind === "text"
          ? message.template
          : Object.values(message.variants).join(" / ");
      return [key.slice(prefix.length), text] as const;
    });

describe("F06 wording of the chart screen", () => {
  test("the chart's own messages use 'chart' and 'combined items', never 'visualization', 'graph' or 'group'", () => {
    for (const [key, text] of shown("en"))
      expect(text, key).not.toMatch(
        /visuali[sz]ation|\bgraph|\bgroup|\bmembers?\b/i,
      );
    for (const [key, text] of shown("ko"))
      expect(text, key).not.toMatch(/시각화|그룹|구성원/);
  });

  test("pages of rows are called ranges in English and 구간 in Korean", () => {
    const ko = Object.fromEntries(shown("ko"));
    const en = Object.fromEntries(shown("en"));
    expect(ko.nextRange).toBe("다음 구간");
    expect(ko.previousRange).toBe("이전 구간");
    expect(ko.loadRangeFailed).toBe("이 구간을 불러오지 못했습니다.");
    expect(ko.noMembers).toBe("이 구간에 항목이 없습니다.");
    expect(en.noMembers).toBe("No items in this range.");
  });

  test("combined items read the same in both languages' fallbacks and bundles", () => {
    const ko = Object.fromEntries(shown("ko"));
    const en = Object.fromEntries(shown("en"));
    expect(en.viewMembers).toBe("View combined items");
    expect(en.viewMembersOf).toBe("View items in {name}");
    expect(ko.viewMembersOf).toBe("{name}의 항목 보기");
    expect(ko.groupedTotal).toBe("묶음 합계");
    expect(en.backToVisualization).toBe("Back to chart");
    expect(ko.backToVisualization).toBe("그래프로 돌아가기");
  });

  test("the in-code English fallbacks match the bundle", () => {
    const en = Object.fromEntries(shown("en"));
    for (const [key, text] of Object.entries(visualizationMessages.en))
      if (!text.includes("{")) expect(text, key).toBe(en[key]);
  });
});
