import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "happy-dom",
    // Hosted runners execute every test file in its own worker on few cores.
    // CPU-bound DOM tests that take about 0.4 s locally have taken 5 s there,
    // so CI gets a wider margin while local runs keep the 5 s default.
    testTimeout: process.env.CI ? 15_000 : 5_000,
    hookTimeout: process.env.CI ? 30_000 : 10_000,
    coverage: {
      provider: "v8",
      reportOnFailure: true,
      reporter: ["text", "lcov", "json-summary"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/ipc/generated/**"],
    },
  },
});
