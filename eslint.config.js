import eslint from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      ".claude/worktrees/**",
      "dist/**",
      "output/**",
      "node_modules/**",
      "coverage/**",
      "local-evidence/**",
      "work/**",
      "**/target/**",
      "tools/footprint-control/target/**",
    ],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{js,jsx,ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}"],
    rules: {
      "no-restricted-globals": [
        "error",
        ...["confirm", "alert", "prompt"].map((name) => ({
          name,
          message:
            "Use Sage's in-app dialogs; native dialog globals are unsafe in Tauri.",
        })),
      ],
      "no-restricted-properties": [
        "error",
        ...["window", "globalThis", "self"].flatMap((object) =>
          ["confirm", "alert", "prompt"].map((property) => ({
            object,
            property,
            message:
              "Use Sage's in-app dialogs; native dialog globals are unsafe in Tauri.",
          })),
        ),
      ],
    },
  },
  {
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "ImportDeclaration[source.value='react'] > ImportNamespaceSpecifier",
          message:
            "React namespace imports are forbidden; use named or default imports so desktop export validation is explicit.",
        },
        {
          selector:
            "ImportDeclaration[source.value='react/jsx-runtime'] > ImportNamespaceSpecifier",
          message:
            "React namespace imports are forbidden; use named JSX runtime imports so desktop export validation is explicit.",
        },
      ],
    },
  },
  {
    files: ["**/*.{js,mjs,cjs}"],
    languageOptions: {
      globals: globals.node,
    },
  },
  {
    files: [
      "tools/footprint-control/dist/**/*.js",
      "tools/footprint-control/frontend/**/*.js",
    ],
    languageOptions: {
      globals: globals.browser,
    },
  },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      "@typescript-eslint/consistent-type-imports": "error",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);
