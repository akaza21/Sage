import { expect, it } from "vitest";
import { sessionAccessModeCandidates } from "./sessionAccessMode";
it("preserves native policy labels and descriptions without adding unsupported policies", () => {
  const capabilities = [
    {
      id: "ask" as const,
      label: "Ask each time",
      description: "Review each permission request; no filesystem sandbox.",
    },
  ];
  expect(sessionAccessModeCandidates(capabilities)).toEqual([
    {
      id: "ask",
      label: "Ask each time",
      description: "Review each permission request; no filesystem sandbox.",
    },
  ]);
});
it("does not invent unsupported access modes", () => {
  expect(sessionAccessModeCandidates(undefined)).toEqual([]);
});

it("rejects malformed IPC label and description values", () => {
  expect(
    sessionAccessModeCandidates([
      { id: "ask", label: 42, description: "review" },
      { id: "ask", label: "Ask", description: null },
    ] as never),
  ).toEqual([]);
});
