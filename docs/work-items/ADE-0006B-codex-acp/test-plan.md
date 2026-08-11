# ADE-0006B test plan: Codex ACP

Status: Active Build evidence; Not Ready; artifact admission and release Test
remain unauthorized.

## Provenance and launch

- Verify exact source/tag/commit, per-file artifact hashes, signatures or
  recorded absence, SBOM, Apache license/NOTICE, Node-or-standalone runtime,
  Codex/App Server identity, platforms, and reproducible source-to-binary map.
- Reject changed/missing artifacts, symlink/reparse replacement, writable
  dependency trees, PATH/`npx`/shell launch, ambient runtime, runtime download,
  normal Codex home, unexpected environment/secrets, and post-check launch
  replacement.
- Reproduce the complete manifest in two clean environments and prove the
  adapter's ranged Codex/ACP SDK dependencies have been replaced by the exact
  admitted graph.
- Prove isolated auth/state ACLs, size/retention, corruption/expiry/logout
  behavior, and no secret or token reaches logs, SQLite, evidence, or WebView.
- Reproduce the tagged lock resolution independently, retain only the selected
  platform closure, and compare the exact selected Codex artifact's generated
  stable App Server schema with the adapter's checked-in/generated types.
  Current OpenAI `main` documentation is reference research and cannot replace
  the pinned artifact/schema fixture.

## Shared contract conformance

- Exercise initialize, capability intersection, bounded model/config pages,
  session new/resume/load/close, prompt/cancel, restart, one recovery attempt,
  explicit close, and unsupported-capability states.
- Saturate every reachable adapter/ACP SDK/App Server reader, queue, pending
  registry and task path independently of Sage's outer caps; reject unbounded
  paths and automatic replay of unresolved effectful requests.
- Map every supported text/thought-summary/plan/tool/result/file/diff/web/MCP/
  subagent-metadata/usage/permission/error update into the shared Items and
  generic UI; unknown events remain bounded unsupported Items.
- Verify ordering, fragmented/coalesced/oversized/malformed frames, duplicate
  and conflicting terminals, late deltas, foreign IDs/runtime epochs,
  request-resolution races, process exit, stdout noise, and stderr flood.
- Prove permission requests are durably declined, no Allow action exists, and
  failure to journal or enqueue a decline tears down the tree.
- Test canonical multi-root ResourceRef mapping, duplicate relative paths,
  nested-root ambiguity, foreign/missing paths, casing aliases, and escape.
- Attempt edits, shell actions, outside-root access, client MCP/tool escalation,
  and malformed/oversized/unknown subagent `_meta`; none may escape read-only
  policy or enable Sage offloading.
- Exercise zero-byte/partial/unknown writes, `deliveryUnknown`, explicit linked
  resend, cancel races, crash/restart, no prompt replay, and projection
  commit-before-notify.

## Containment, resources, and regressions

- On every claimed platform test verified image identity, fixed arguments,
  process/descendant caps, parent death, cancellation/deadline, PID reuse,
  teardown, and the exact admitted zero-orphan or trusted-process guarantee.
- Measure first-token, native receipt-to-paint, cancel dispatch/terminal,
  warm/cold wake, queue saturation/control-lane latency, CPU/RSS/process count,
  auth/state/database/WAL disk, package/install delta, and teardown P50/P95.
- Run five clean and five failure cycles, the complete ADE-0006A adversarial
  fixture, accessibility/CSP/security/policy checks, and all ADE-0001-5
  regression gates using isolated state.

Exact commands, counts, hashes, machine identity, limitations, and raw-evidence
provenance belong in a later `results.md`. Build evidence is followed by fresh
Review and Independent Test; it is not acceptance.

## Current Build evidence (2026-08-11)

The fail-closed packet slices exercise exact Windows package inventory
metadata matching, schema/source/package claim drift rejection, declared
state/package separation (including equality and both containment directions),
single declared package-root enforcement, real initialize shape and Sage
identity validation, packet-owned capability projection, and the unchanged
`ArtifactNotAdmitted` production-launch gate.

The Windows physical-closure seam additionally exercises a bounded small-file
stand-in and an explicitly environment-gated run over the offline-reproduced
exact eight-file closure: exact configured package-root
mapping; lossless native path identity; exact stored component spelling;
canonical final handle paths; regular-file, size, SHA-256, aggregate, and
closed-set enforcement; 64-entry/depth-8 exact and plus-one bounds;
missing/extra/changed files; hard-link rejection; deterministic reparse-attribute
rejection plus an OS-permitted symlink smoke; existing-writer and post-hash
identity races; a late namespace entry between bounded observations; and
component-by-component, no-follow canonical disjoint state, including a
dangling directory symlink, without creating or mutating state. Non-Windows
inspection remains unavailable. The production inspector processed the real
427,151,500-byte package and retained its handles on the implementation
workstation. These tests prove verifier behavior and one developer
reproduction, not independent reproduction or admission. Retained
handles do not freeze the directory namespace, which can change after the final
observation. The tests do not prove Authenticode/signatures, SBOM/license
closure, ACLs/ownership/immutability, provisioning, auth/state containment,
process/network containment, atomic check-to-launch, or macOS admission.

The exact source inspection and minimal-environment executable run are recorded in
[initialize evidence](initialize-0.145.0-build-evidence.md). The focused
command is:

```text
cargo test --lib acp::bindings::codex
```

It passed 55 default tests on the implementation workstation. One additional
ignored test now requires both the exact managed package and existing isolated
state roots:

```text
cargo test --lib acp::bindings::codex::tests::exact_registry_package_and_state_satisfy_managed_prerequisite_without_launch_authority -- --ignored --exact --nocapture
```

The resulting 56 focused test declarations include malformed, unknown-field,
exact-limit-plus-one, registry/verification-boundary drift, mismatched physical
package-root and isolated-state-root binding, mandatory-constructor, and
real-package/state fail-closed coverage. The ignored gate is unrun because the
managed roots are absent. This is
developer self-check evidence, not the clean-machine package reproduction, signature
verification, hostile containment probe, fresh Review, or Independent Test
required for Ready.

The reproduction-prerequisite increment requires exact raw archive and
registry identities and preserves unresolved facts as typed fail-closed
boundaries: npm signature and SLSA presence are unverified, Authenticode is a
workstation observation, license/NOTICE closure is incomplete, the SBOM is
unavailable, and ACL/ownership/immutability are unverified. The prerequisite
must consume a physical proof for the same packet and is mandatory at runtime
construction, but it grants no artifact admission or launch authority. Exact
commands, results, and observed identities are in the
[package-reproduction evidence](package-reproduction-0.145.0-build-evidence.md).

The managed-staging prerequisite adds a 64-node, 64-KiB-per-descriptor,
1-MiB-aggregate, and 256-ACE bounded Windows security pass. Tests cover
package and state owner policy, protected-root DACLs, current-token effective
write denial/allowance, conservative foreign mutating grants, exact and
plus-one bounds, different package/state binding, and descriptor/owner/access/
token drift. The safe-slice DACL parser additionally proves the exact
256-accepted/257-rejected ACE boundary and rejects truncated ACL/ACE headers,
declared-extent overflow, undersized standard ACEs, malformed SID extents, and
all object/callback/compound/unknown ACE layouts before native SID validation.
A native smoke proves an ordinary user-owned writable file fails the package
policy. Production runtime construction now requires this typed prerequisite
and repeats the exact closure plus security snapshot before still returning
`LaunchUnavailable` at `ArtifactNotAdmitted`.

The renamed ignored gate requires both `SAGE_CODEX_0145_PACKAGE_ROOT` and
`SAGE_CODEX_0145_STATE_ROOT` and is documented in the
[managed-staging evidence](managed-staging-0.145.0-build-evidence.md). It was
not run because the exact managed roots are absent. Thus the verifier is local
green, while provisioning, exact-root ACL evidence, global immutability, and
atomic check-to-launch remain unproven.

The sealed-staging increment adds bounded Windows fixtures for an arbitrary
exact offline source, source hard-link rejection, exact-plus-one path rejection,
target collision, one-live-stage exclusion, copy-failure rollback, exact
receipt/file/target/ACL-plan contents, post-write receipt drift, revalidation,
drop cleanup, and typed ACL/publish unavailability. Its ignored production test
requires `SAGE_CODEX_0145_OFFLINE_SOURCE_ROOT`, the exact value
`SAGE_CODEX_0145_STAGE_ONLY=prepare-sealed-offline-stage-no-publish`, absent
canonical targets, and the existing canonical package parent. The local test
does not apply ACLs, publish either target, alter credentials, launch Codex, or
claim recovery of a crash-abandoned stage; those operations belong to the
separately reviewed privileged-helper increment documented in the
[sealed-staging evidence](provisioning-staging-0.145.0-build-evidence.md).

The security-prerequisite increment adds exact and drift fixtures for the
packet-bound isolated `CODEX_HOME`, file credential store, disabled ambient
config and update checks, one-live-tree ceiling, cleared five-key environment,
zero secret keys, shared environment byte ceilings, and thread/resume/turn
read-only request fields. It rejects normal-home, ambient config/environment,
added secret or `PATH`, malformed/unknown policy fields, changed values, and
both numeric and document-size exact-limit-plus-one cases. A valid prerequisite
still returns `LaunchUnavailable` through the unchanged
`ArtifactNotAdmitted` production gate, and the catalog assertion keeps Codex
`notInstalled`.

No test in these increments proves directory provisioning or the exact
candidate's applied ACLs, an adequate or
enforced Codex writable-state disk/retention budget, authentication lifecycle
or redaction, endpoint/proxy/TLS/offline behavior, telemetry, Windows
containment, or launch authority. Those remain H1-03/H1-04 admission work.
