import { expect, it } from "vitest";
import {
  sessionAccessModeCandidates,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { chatAccessCandidates, chatAccessEffective } from "./chatAccessMode";

const claude: SessionAccessModeCapability[] = [
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
    fallback: true,
    sessionModeId: "default",
  },
  {
    id: "acceptEdits",
    label: "Accept edits",
    description: "File edits run without asking.",
    sessionModeId: "acceptEdits",
  },
  {
    id: "plan",
    label: "Plan",
    description: "Claude plans without changing files.",
    sessionModeId: "plan",
  },
  {
    id: "auto",
    label: "Auto",
    description: "Claude runs what it judges safe.",
    sessionModeId: "auto",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
    sessionModeId: "bypassPermissions",
  },
];
const ids = (candidates: readonly SessionAccessModeCapability[]) =>
  candidates.map((candidate) => candidate.id);

it("offers only the modes the live session advertises", () => {
  const advertised = ["default", "acceptEdits", "auto"];
  expect(ids(chatAccessCandidates(claude, advertised))).toEqual([
    "ask",
    "acceptEdits",
    "auto",
  ]);
});

it("offers Auto and Full access only to a live session, never as a default", () => {
  expect(ids(sessionAccessModeCandidates(claude))).toEqual([
    "ask",
    "acceptEdits",
    "plan",
  ]);
  expect(ids(chatAccessCandidates(claude, null))).toEqual([
    "ask",
    "acceptEdits",
    "plan",
  ]);
});

it("runs a chat in its own mode and every other chat at the default", () => {
  const candidates = chatAccessCandidates(claude, [
    "default",
    "acceptEdits",
    "plan",
    "bypassPermissions",
  ]);
  expect(chatAccessEffective("acceptEdits", candidates, "fullAccess")).toBe(
    "fullAccess",
  );
  expect(chatAccessEffective("acceptEdits", candidates, "plan")).toBe("plan");
  expect(chatAccessEffective("acceptEdits", candidates, null)).toBe(
    "acceptEdits",
  );
  expect(chatAccessEffective(undefined, candidates, undefined)).toBe("ask");
  expect(chatAccessEffective("plan", candidates, "auto")).toBe("plan");
});
