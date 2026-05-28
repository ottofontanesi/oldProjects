import { defineWorkspace } from "vitest/config";

export default defineWorkspace([
  {
    extends: "./vite.config.ts",
    test: {
      name: "unit",
      include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
      exclude: ["src/**/backtest-*.test.ts"],
    },
  },
  {
    extends: "./vite.config.ts",
    test: {
      name: "backtest",
      include: ["src/**/backtest-*.test.ts"],
    },
  },
]);
