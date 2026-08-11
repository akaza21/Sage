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

The elevated-helper core increment adds request exact-limit/plus-one and
truncation cases; secret, authenticator, helper version/hash/byte drift; nonce,
source-proof, manifest order/hash/byte, target, and ACL drift; authenticated
journal truncation/plus-one/tamper and replay/collision cases; and failpoints
after atomic initial journal creation, native state publication, state phase
persistence, package publication, package phase persistence, handoff removal,
and stage-root removal. It also proves convergent rollback for ACL,
publication, and final-proof failures, including a vanished canonical state
observation, plus journal-retaining retry through each bounded post-commit
cleanup prefix. The concrete Windows adapter is
compiled and routes ACL writes through handles whose volume/file identities
match the retained no-follow inspection, then uses the existing managed ACL
inspector for stage/state/final proof. No test manufactures an elevated-process
capability. A Windows lock fixture also proves a second helper owner is rejected
and a post-drop recovery owner can reacquire. Native adversarial fixtures prove
same-user state replacement is rejected against the journaled file identity;
destination-parent and state-effect authorities deny external rename; package
publish/rollback uses a revalidated exact source; a cross-nonce torn fixed
journal temp recovers through atomic no-replace publication; the narrow
source-declared `SetSecurityInfo` ABI preserves native error results; and
stage/cleanup enumeration rejects exact-limit plus one without an unbounded
collection. See the
[helper-core evidence](provisioning-helper-0.145.0-build-evidence.md).

The feature-only helper process-boundary increment additionally exercises
fixed/truncated/trailing/tampered request and response frames, wrong request
digest replay, caller PID/SID/elevation and helper-elevation decisions, exact
helper hashing while its image handle denies write/delete replacement,
hard-link and empty-image rejection, and the fixed bounded pipe namespace. A
live ordinary-user integration run must return the sealed
`ElevationRequired` status, converge the child, reject a second connection,
preserve the helper bytes, and leave no temp files beyond the copied fixture.
An unconnected helper must terminate at its own deadline and remove its pipe.
Policy must prove the helper requires its explicit Cargo feature while default
features and all Tauri resource/external-binary inventories remain empty. This
is an inert transport test only: no test may construct `ElevatedHelperProcess`,
invoke the mutation core, trigger UAC, or publish canonical roots. See the
[process-boundary evidence](provisioning-helper-boundary-0.145.0-build-evidence.md).

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

The registration increment additionally proves that production startup has a
typed `IndependentAdmission` stop, that only a private test authority can mint
the opaque non-Clone admitted Binding, that the Binding revalidates its exact
transitive package/security/state/environment and immutable identity proofs,
and that `AcpService` and `HarnessCatalog` share one atomically invalidating
registration. A test-only admitted registration can light the catalog while
exposing no invented models; the normal startup registration cannot. A
post-registration physical-state drift test proves one explicit bounded refresh
makes the catalog unavailable and prevents the service from constructing a
runtime. Concurrent refreshes converge on the same typed `Drift` state. A
`Weak` lifetime proof shows that an admitted runtime retains the Binding and
frozen launch environment until runtime teardown, but not afterward.

The admitted-launch-contract increment adds focused Windows tests proving that
only an admitted runtime can produce the shared supervisor input; the contract
contains the exact entrypoint identity, one `app-server` argument, runtime
epoch, a registry-resolved ProjectGeneration-qualified single-root authority,
enforced tree containment, and exactly
`CODEX_HOME`, `SystemRoot`, `TEMP`, `TMP`, and `WINDIR`. It proves the absence
of inherited `HOME`, `USERPROFILE`, `PATH`, and `OPENAI_API_KEY`; rejects wrong
project, stale generation, wrong/unknown root, missing/noncanonical and
symlink/junction aliases; accepts exactly one registered root and rejects two
before runtime/process admission; proves a same-ID/different-directory
authority cannot replace the exact non-Clone authority consumed by the runtime;
detects or blocks directory replacement between contract creation and spawn;
and fails closed after Binding drift. The argument-free launch handoff is
one-shot. Characterization fixture multi-root behavior stays on its separate
generic selected-root path. The existing shared-registration refresh remains
the bounded transition that invalidates service and catalog availability.

The contract test deliberately does not spawn. The generic Windows supervisor
remains the only executor and reopens/hashes the exact image, revalidates the
retained root handle plus path identity, and enforces the Job boundary at
spawn. A real ignored
Codex campaign was not run because `SAGE_CODEX_0145_PACKAGE_ROOT` and
`SAGE_CODEX_0145_STATE_ROOT` are unset and both canonical managed roots are
absent. No model, Turn, restart, RSS, WAL, latency, or admission claim follows
from these local tests.
[Build evidence](live-runtime-launch-contract-0.145.0-build-evidence.md)

No test in these increments proves a live elevated executable/UAC boundary or
the exact candidate's applied canonical-root ACLs, an adequate or
enforced Codex writable-state disk/retention budget, authentication lifecycle
or redaction, endpoint/proxy/TLS/offline behavior, telemetry, Windows
containment, or launch authority. Those remain H1-03/H1-04 admission work.
