import { ESLint } from "eslint";
import { expect, test } from "vitest";

test(
  "feature views can use layout elements but must register all UI primitives",
  // Loading the real ESLint configuration on a cold Windows runner can exceed 5 seconds.
  { timeout: 20_000 },
  async () => {
    const eslint = new ESLint();
    const [result] = await eslint.lintText(
      'import {Button} from "react-aria-components"; export const Invalid = () => <section><Button /><button>Raw</button></section>;',
      { filePath: "src/features/analysis/UnregisteredView.tsx" },
    );
    expect(result?.messages.map(({ ruleId }) => ruleId)).toEqual([
      "no-restricted-imports",
      "no-restricted-syntax",
    ]);
  },
);
