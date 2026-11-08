import { describe, expect, it } from "vitest";

import { sessionStatusRows } from "./sessionStatusRows";

describe("sessionStatusRows", () => {
  it("leaves out what the session has not reported", () => {
    expect(
      sessionStatusRows({
        model: { model: "GPT-5.5", effort: null },
        access: null,
      }),
    ).toEqual([["Model", "GPT-5.5"]]);
    expect(sessionStatusRows({ model: null, access: undefined })).toEqual([]);
  });

  it("adds the account, plan and usage windows when they are reported", () => {
    expect(
      sessionStatusRows({
        model: { model: "GPT-5.5", effort: "High" },
        access: "Ask",
        account: "ChatGPT",
        usage: {
          plan: "Pro",
          windows: [
            { label: null, usedPercent: 41.6, windowMinutes: 300 },
            { label: null, usedPercent: 7, windowMinutes: 10080 },
            { label: "Reviews", usedPercent: 0, windowMinutes: null },
            { label: null, usedPercent: 3, windowMinutes: 45 },
          ],
        },
      }),
    ).toEqual([
      ["Model", "GPT-5.5"],
      ["Effort", "High"],
      ["Access", "Ask"],
      ["Account", "ChatGPT"],
      ["Plan", "Pro"],
      ["5-hour limit", "42% used"],
      ["7-day limit", "7% used"],
      ["Reviews", "0% used"],
      ["45-minute limit", "3% used"],
    ]);
  });
});
