import { configure } from "@testing-library/react";

// findBy* and waitFor poll for 1 s by default. Hosted runners run CPU-bound DOM
// tests up to ten times slower than a workstation, so CI waits longer for the
// same observable state; local runs keep the default so slow code still shows up.
configure({ asyncUtilTimeout: process.env.CI ? 10_000 : 1_000 });
