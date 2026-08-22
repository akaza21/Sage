import assert from "node:assert/strict";
import test from "node:test";

import {
  FROZEN_ACP_TRANSPORT_FIELDS,
  acpTransportParityErrors,
} from "./acp-transport-parity.mjs";

const root = process.cwd();

test("frozen ACP capability and item identity fields are explicit", () => {
  assert.ok(
    FROZEN_ACP_TRANSPORT_FIELDS.AcpCapabilitySet.includes("schemaVersion"),
  );
  assert.ok(
    FROZEN_ACP_TRANSPORT_FIELDS.AcpCapabilitySet.includes("protocolVersion"),
  );
  assert.ok(FROZEN_ACP_TRANSPORT_FIELDS.AcpItemIdentity.includes("turnId"));
  assert.ok(FROZEN_ACP_TRANSPORT_FIELDS.AcpItemIdentity.includes("lifecycle"));
  assert.deepEqual(FROZEN_ACP_TRANSPORT_FIELDS.BetaEvidenceResult, [
    "candidate",
    "contractSha256",
    "evidence",
    "schemaVersion",
    "verdict",
    "workItem",
  ]);
});

test("Rust and TypeScript ACP and evidence-result contracts match the freeze", async () => {
  const errors = await acpTransportParityErrors(root);
  assert.deepEqual(errors, []);
});
