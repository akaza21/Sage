import { expect, test } from "./test";

export function forbidBrowserRetries() {
  test.beforeEach(({ browserName }, testInfo) => {
    expect(browserName).toBe("chromium");
    expect(testInfo.retry, "browser checks must pass on the first run").toBe(0);
  });
}
