import { expect, it } from "vitest";

import {
  MAX_LISTED,
  admitImportReport,
  admitOverview,
  importSummary,
  siteLabel,
} from "./savedPasswords";

it("admits only the listed fields of a ready overview", () => {
  expect(
    admitOverview({
      status: "ready",
      total: 3,
      matching: 2,
      entries: [
        {
          id: "a",
          origin: "https://a.example",
          username: "ada",
          password: "x",
        },
        { id: 7, origin: "https://b.example", username: "grace" },
      ],
    }),
  ).toEqual({
    status: "ready",
    total: 3,
    matching: 2,
    entries: [{ id: "a", origin: "https://a.example", username: "ada" }],
  });
});

it("treats anything but a ready reply as unsupported and caps the list", () => {
  expect(admitOverview(null)).toEqual({ status: "unsupported" });
  expect(admitOverview({ status: "other" })).toEqual({ status: "unsupported" });
  const entries = Array.from({ length: MAX_LISTED + 10 }, (_, index) => ({
    id: String(index),
    origin: "https://a.example",
    username: "",
  }));
  const overview = admitOverview({ status: "ready", total: -1, entries });
  expect(overview).toMatchObject({ total: 0, matching: 0 });
  expect(overview.status === "ready" && overview.entries).toHaveLength(
    MAX_LISTED,
  );
});

it("reports an import in counts only", () => {
  expect(admitImportReport(null)).toBeNull();
  const report = admitImportReport({
    added: 4,
    updated: 1,
    unchanged: 0,
    skipped: 2,
    fileName: "Chrome Passwords.csv",
  });
  expect(report && importSummary(report)).toBe(
    "Imported Chrome Passwords.csv: 4 added, 1 updated, 2 skipped.",
  );
});

it("labels HTTPS sites by host and keeps other origins whole", () => {
  expect(siteLabel("https://example.com")).toBe("example.com");
  expect(siteLabel("https://shop.example:8443")).toBe("shop.example:8443");
  expect(siteLabel("http://localhost:8080")).toBe("http://localhost:8080");
});
