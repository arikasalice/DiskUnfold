import tseslint from "typescript-eslint";

const nativeBoundary = {
  selector:
    "Identifier[name=/^(__diskunfoldRequest|messageHandlers|webkit|chrome)$/]",
  message: "Use the typed IPC client for native webview access.",
};

export default [
  {
    ignores: [
      "dist/**",
      "preview-dist/**",
      "coverage/**",
      "src/ipc/generated/**",
    ],
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
      "no-restricted-syntax": ["error", nativeBoundary],
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/components/**", "src/ipc/bridge.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: [
                "react-aria-components",
                "react-aria-components/*",
                "lucide-react",
                "lucide-react/*",
              ],
              message:
                "Register UI primitives in src/components and import them from there.",
            },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        nativeBoundary,
        {
          selector:
            "JSXOpeningElement > JSXIdentifier[name=/^[a-z]/]:not([name=/^(main|header|section|div|aside|footer|nav|article)$/])",
          message:
            "Use a common component for every element that is not layout.",
        },
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "Render display values through common components.",
        },
      ],
    },
  },
];
