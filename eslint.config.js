import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      ".devin/**",
      "**/dist/**",
      "**/node_modules/**",
      "**/src-tauri/gen/**",
      "target/**",
      "workspace-blueprint/**",
    ],
  },
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    files: ["scripts/*.mjs"],
    languageOptions: {
      globals: { console: "readonly", process: "readonly" },
    },
  },
);
