import tseslint from "typescript-eslint";

export default [
  {
    ignores: ["dist/**", "coverage/**", "src/ipc/generated/**"],
  },
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    rules: { complexity: ["error", 20] },
  },
];
