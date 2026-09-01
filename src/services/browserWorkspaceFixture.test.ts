import { expect, it } from "vitest";

import {
  BROWSER_FIXTURE_LABEL,
  browserRepository,
} from "./browserWorkspaceFixture";

it("labels browser repository fixtures as simulated state", () => {
  expect(BROWSER_FIXTURE_LABEL).toMatch(/simulated.*not live/i);
  expect(browserRepository.roots[0].message).toBe(BROWSER_FIXTURE_LABEL);
});
