import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["apps/*/src/**/*.test.ts", "apps/*/src/**/*.test.tsx"],
  },
});
