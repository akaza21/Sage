# ADE-0006A C0-S contract packet

Status: the 2026-08-09 Package comparator, ADE-wide campaign lock, and
renderer-recreation observer amendments are user-approved and awaiting fresh
named review. These files admit no harness by themselves.

This directory makes the C0-S proposal mechanically reviewable:

- `schema-005.sql` is the exact proposed final SQLite schema. It is not the
  executable migration body; bounded legacy conversion and crash-safe database
  replacement remain implementation work after the gates close.
- `limits-v1.json` is the single numeric count, byte, queue, process, storage,
  native-thread projection, fixture, and evidence ledger.
- `limit-fixture-coverage-v1.json` binds enforced wire/process/projection limits
  to literal exact and limit-plus-one fixture IDs and orthogonality rules.
- `migration-recovery-v1.json` freezes backup publication, crash adoption,
  v5 replacement, offline restore, and failpoint oracles.
- `c1-characterization-v1.json` bounds the sole pre-C0-I source exception and
  states exactly what C0-I inherits or removes.
- `request-digest-vectors-v1.json` supplies executable SHA-256 known-answer
  vectors for absent/null/whitespace/order/escape/numeric distinctions.
- `fixture-vectors-v1.json` lists every initial adversarial case and oracle.
- `packaged-gestures-v1.json` lists the semantic packaged interaction matrix.
- `package-contract-v1.json` freezes the Windows production-default EXE/MSI/
  NSIS artifact set, the tracked ADE-0005 frontend-size baseline, WebView2
  provisioning, fixture exclusion, observer bounds, and required artifact
  binding.
- `performance-budgets-v1.json` fixes proposed C0-S latency, resource,
  persistence, package-delta, and sampling ceilings before C1 measurement.
- `transcript-side-chat-v1.json` records bounded transcript annotations, temporary
  Chat ownership, supervisor teardown, preferences, and unexecuted checks.
- `protocol-contract-v1.json` owns lifecycle transitions, digest version 1,
  revision-only notification, IPC intent, and dormant native-thread semantics.
- `evidence-policy-v1.json` owns the ADE-wide Package/C0-B admission lock,
  evidence bounds, renderer-recreation evidence, redaction, provenance, and
  independence rules.
- `dependency-admission-v1.json` records the schema-only selection, rejected
  runtime, Windows API features, and evidence still required for admission.
- `ownership-manifest-v1.json` is the exact C0-S/C0-I/P1/P2/P3/final-wiring
  pathname allowlist and merge order.
- `contract-manifest-v1.json` binds all files above by SHA-256 and bytes.

The manifest intentionally excludes itself, so regenerating it is not
self-referential. Any content change requires updating the manifest and a new
fresh-context review. Markdown briefs may explain these files but cannot relax
their limits, add an owned path, enable a capability, or change a test oracle.

The checkpoint sequence is:

```text
ADE-0005 Gate 0
  -> C0-S specification freeze
  -> C0-A dependency admission
  -> serialized C1 characterization
  -> Independent C1 identity acceptance
  -> serialized C0-I shared-contract implementation
  -> fresh P1/P2/P3 worktrees
  -> serialized integration and C0-B comparison
  -> Review and Independent Test
```

The native-thread fields are dormant in ADE-0006A. They let later adapters
faithfully project harness-created child activity and proven native fork/steer
semantics without giving Sage authority over harness-owned worker scheduling.
