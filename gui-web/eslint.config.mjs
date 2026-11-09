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
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/ipc/bridge.ts"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "Identifier[name=/^(__diskunfoldRequest|messageHandlers|webkit|chrome)$/]",
          message: "Use the typed IPC client for native webview access.",
        },
      ],
    },
  },
];
