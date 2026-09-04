# Browser visual regression lane

This developer-only Playwright lane protects the Wave 2 browser-rendered UI.
It loads the normal Vite application and then installs bounded, deterministic
fixture state through `fixtures/visualScenarios.ts`. The fixture is not imported
by production code and does not replace native acceptance.

## Structure (Q-07 surface split)

The Q-07 split kept the existing test titles, screenshot names, snapshot
directory (`workbench.visual.spec.ts-snapshots/`), clock/locale/font/reflow
settlement, and accessibility/viewport/DPR/reduced-motion behavior from the
pre-split `workbench.visual.ts` + `agent-resource-shell.visual.ts` pair. AV-03
extends the lane to 59 tests with two named Agent-ready baselines; no existing
baseline PNG changed.

| Surface | Playwright spec | Fixture owner |
| --- | --- | --- |
| Agent | `agent.visual.ts` (conversation, dual/single/long Chat, activity, image, child activity, Agent accessibility/responsive states) | `fixtures/surfaces/agentVisualFixtures.ts` |
| Editor shell | `editor.visual.ts` (Explorer, Find, Settings, Source Control, welcome, Editor narrow/forced-colors/zoom) | `fixtures/surfaces/editorVisualFixtures.ts` |
| Editor tools | `editor-tools.visual.ts` (Search, Problems, Outline, control states, focus/selection/resizer language, row containment at zoom) | `fixtures/surfaces/editorVisualFixtures.ts` |
| Resources | `resource.visual.ts` (File, Diff, Preview, the folded V6a right-resource shell, artifact follow/pin, cross-mode parity) | `fixtures/surfaces/resourceVisualFixtures.ts` |
| Recovery | `recovery.visual.ts` (failed/error and waiting/running Turn states) | `fixtures/surfaces/recoveryVisualFixtures.ts` |

Shared code is limited to parameterized, mode-neutral browser primitives in
`shared/visualBrowserPrimitives.ts` (fixed clock, workbench layout,
navigation, font/reflow settlement, transcript settlement,
viewport-overflow probe, canonical screenshot). It owns no scenario
registry, no mode branches, no landmark or surface assertions, and
registers no tests. Each surface owns its registry slice, landmarks,
branches, and probes in a co-located helper: `agentVisualHelpers.ts`
(Agent landmarks, activity/image/child-activity assertions, Agent
canonical runner), `editorVisualHelpers.ts` (Editor landmarks, Find
containment, Editor canonical runner), and `resourceVisualHelpers.ts`
(Agent-mode scenario opener, File/Diff resource-state installers, and
containment probes). Each spec
file iterates the authoritative `fixtures/visualScenarioCatalog.ts`
catalogue filtered to its slice and registers its own `test()` entries,
so Playwright attribution stays with the owning lane. Fixture truth is
layered as `fixtures/visualScenarioTypes.ts` (scenario names, modes,
and the disjoint surface slices), `fixtures/visualScenarioCatalog.ts`
(ordered catalogue plus `scenarioSurface`), `fixtures/visualFixtureIds.ts`,
`fixtures/visualItemBuilders.ts`, `fixtures/visualChatPages.ts`,
`fixtures/visualDocumentFixtures.ts`, `fixtures/visualWorkbenchState.ts`, and
`fixtures/visualScenarioShell.ts` (scenario dispatch plus workbench install);
Editor-tool installation lives in
`fixtures/surfaces/editorVisualFixtures.ts`.
`fixtures/visualScenarios.ts` is a single-route compatibility entrypoint
exposing only the dynamically consumed `installVisualScenario`; every
other member is imported from its authoritative module directly.
`fixtures/visualScenarioOwnership.test.ts` proves the slices are explicit,
disjoint, complete, deterministic, and duplicate-free, and asserts the
barrel's exact export-key allowlist so future leakage fails.

## Commands

```powershell
npx playwright install chromium
npm run test:ui
npm run test:ui:update
```

`test:ui` compares against the tracked Windows Chromium baselines.
`test:ui:update` rewrites those baselines and should be used only after visually
reviewing the intended change. Failure screenshots and Playwright error context
are written below `output/playwright/`, which is ignored by Git.

The lane runs 59 tests serially with a fixed clock, locale, timezone, font wait,
disabled screenshot animations, hidden carets, and explicit transcript
follow-end settlement. Screenshots are scoped to `main.workbench`; role
assertions verify important landmarks and controls before capture. Every
responsive case also rejects document-level horizontal overflow.

## Coverage

| State | Browser proof |
| --- | --- |
| Agent | Single Chat, dual Chat, measured-width focused-Chat fallback, a bounded 14-turn transcript, the complete Thought/Plan/tool/usage/error/unknown activity family, a bounded accessible inline image, and a keyboard-disclosed read-only child-activity hierarchy across levels one through four |
| Editor | Explorer plus active-file restoration inside its real scroll viewport, a compact selection-aware More menu with keyboard/dialog focus handoff and plain unavailable reasons, the functional welcome surface, compact in-file Find, Settings, populated/empty Search, Source Control with repository identity, one state-derived primary action, loading/error and complete control-interaction states, populated/empty Problems and Outline with exact mouse/keyboard source navigation and 200%-zoom row containment, tabs, breadcrumbs, CodeMirror, dirty/save status, status bar, and one restrained translucent highlight language across focus, selection, active controls, and the resizer |
| Resources | Agent File, Agent Diff, safe inline image, loopback Preview, one truthful collapsed resource toggle, expanded empty plus announced File/Diff loading/error states, cross-mode File/Diff token-authority parity (computed canvas/header/panel/code/gutter/divider/active-line/caret/selection/add/delete-foreground/muted/syntax roles plus same-size document-region pixels), Agent file-ready/diff-ready states, and exact per-Chat Follow latest / pinned-resource restoration |
| Recovery | Failed Turn and waiting/running Turn |
| Accessibility | Forced colors, reduced motion, keyboard focus-visible, keyboard-opened Agent resources with focus return, announced resource status/alerts, shared 1px translucent Agent/Editor focus edges with restrained halos and no stacked CodeMirror outlines, retained 2px system `Highlight` focus in forced colors, Explorer menu traversal and focus return, full accessible child-activity labels, and disclosure-only hierarchy navigation |
| Responsive | 720x560 narrow window and a 1380x880 device emulated at 200% zoom, including empty/File/Diff Agent resource states, compact Explorer action-menu and four-level child-activity containment |

The zoom case uses Chromium device metrics with a 2x scale factor and asserts
both a 690 CSS-pixel effective width and `devicePixelRatio === 2` before capture.
It therefore protects the same reflow boundary as a 1380x880 window at 200%,
while the native walkthrough remains responsible for WebView integration.

## Dependency and runtime impact

`@playwright/test` 1.62.1 is an exact, dev-only dependency. Its three installed
npm packages have approximately 18.5 MB of unpacked JavaScript in total and are
not part of the Vite production bundle. Chromium installation is separate from
`npm ci`; on the Windows authoring machine the Chromium and headless-shell cache
occupied 731,309,635 bytes (about 697 MiB) outside the repository. The 42
integrated-candidate PNG baselines occupy 2,560,893 bytes.

After the Advisor return, the technical owner approved a bounded AV-04 revision
that restored the two-pixel idle splitter pseudo-lines while retaining the
one-pixel normal keyboard-focus edge. Its focused Windows run passed 5/5 in 6.5
seconds, and the final full run passed 59/59 in 49.6 seconds. This lane is
purposely not included in
`npm run check`: the existing CI matrix is not a Windows pixel environment, and
the browser download is materially larger than the normal frontend check. A
future merge gate should run `npm run test:ui` on pinned Windows Chromium
infrastructure.

AV-03 adds `agent-file-ready` and `agent-diff-ready` Windows baselines plus a
same-size document-region pixel comparison between Agent and Editor (locator
clips, exact buffer equality, no new pixel library or tolerance exception).
Existing `file`/`diff` full-workbench baselines keep their meaning and stay
byte-identical because token values are unchanged.

AV-04 adds one Node policy file (43 → 44 files) containing seven dependency-free
cases for normal, multiline, and color-first thick rings; nested forced-colors
exceptions; comment/string false positives; allowed one-pixel/soft-halo
declarations; file/byte bounds; and cap-plus-one traversal settlement. The file
took 135.3 ms in the final 2026-09-04 WSL budget invocation, and the policy is
part of `check-policy.mjs`. Its browser evidence extends the existing Agent
keyboard-focus and Editor control-state checks with computed resolved shared
edge/soft-halo styles and actual forced-colors keyboard focus; it adds no
Playwright scenario. The final focused post-review Windows verifier passed 5/5.
The final candidate is based on the reviewed baseline-only AV-04N normalization
commit `916a918`; inherited branch-label and V6a resource-rail pixels are not
attributed to AV-04.
The ordinary update wrapper did not rewrite the below-tolerance divider drift,
so the orchestrator used `--update-snapshots=all` for only the two authorized
tests and inspected their exact changed regions; screenshot tolerance and
configuration remain unchanged. `agent-keyboard-focus` is SHA-256 `1836e625…`
at 67,668 bytes: compared exactly with `916a918`, its 12,196 changed pixels are
bounded to x479–1113/y695–850, with zero outside and zero on dividers.
`agent-child-activity` is SHA-256 `fd4fede8…` at 74,287 bytes: its 3,948 changed
pixels are bounded to x491–1102/y382–433, also with zero outside and zero on
dividers. `editor-control-states` remains byte-identical at SHA-256 `cf5a5452…`
and 72,550 bytes; Agent forced colors remains `17ef9daa…`, and Editor forced
colors remains `b703b66d…`.

Final WSL evidence is 12/12 focused seams, 7/7 policy cases at 135.3 ms, a
2,089-module build producing 130,330-byte CSS (19,962 gzip) and 615,688-byte
entry JavaScript (170,769 gzip), and 192 Vitest files / 1,189 tests before the
inherited Node-budget boundary (31/44 passed, 13 unrelated Windows-assumption
failures). Production policy reports only the four inherited Rust size
findings.

## Deliberate limits

- The tracked baselines are Windows Chromium only.
- Fixtures exercise real React surfaces and stores but not Tauri commands,
  native dialogs, WebView destruction/recreation, or signed packages.
- Signed-package, macOS, and operating-system assistive-technology acceptance
  remain outside this browser lane.
