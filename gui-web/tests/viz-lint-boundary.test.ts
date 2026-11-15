import { ESLint } from "eslint";
import { expect, test } from "vitest";

test(
  "the visualization model layer gets the same UI registration rules as feature views",
  // Loading the real ESLint configuration on a cold Windows runner can exceed 5 seconds.
  { timeout: 20_000 },
  async () => {
    const eslint = new ESLint();
    const source =
      'import {Button} from "react-aria-components"; export const Invalid = () => <section><Button /><button>Raw</button></section>;';
    const [result] = await eslint.lintText(source, {
      filePath: "src/viz/UnregisteredView.tsx",
    });
    expect(result?.messages.map(({ ruleId }) => ruleId)).toEqual([
      "no-restricted-imports",
      "no-restricted-syntax",
    ]);
  },
);
