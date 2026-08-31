# Browser visual regression lane

This developer-only Playwright lane protects the Wave 2 browser-rendered UI.
It loads the normal Vite application and then installs bounded, deterministic
fixture state through `fixtures/visualScenarios.ts`. The fixture is not imported
by production code and does not replace native acceptance.

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

The lane runs 13 tests serially with a fixed clock, locale, timezone, font wait,
disabled screenshot animations, hidden carets, and explicit transcript
follow-end settlement. Screenshots are scoped to `main.workbench`; role
assertions verify important landmarks and controls before capture. Every
responsive case also rejects document-level horizontal overflow.

## Coverage

| State | Browser proof |
| --- | --- |
| Agent | Single Chat, dual Chat, and a bounded 14-turn transcript |
| Editor | Explorer, tabs, breadcrumbs, CodeMirror, and status bar |
| Resources | Agent Diff, loopback Preview, and empty right pane |
| Recovery | Failed Turn and waiting/running Turn |
| Accessibility | Forced colors, reduced motion, and keyboard focus-visible |
| Responsive | 720x560 narrow window and 690x440 200%-equivalent viewport |

The 690x440 case represents the CSS-pixel space left when the 1380x880
reference window is viewed at 200% zoom. It does not claim native WebView zoom
or operating-system scale-factor acceptance.

## Dependency and runtime impact

`@playwright/test` 1.62.1 is an exact, dev-only dependency. Its three installed
npm packages have approximately 18.5 MB of unpacked JavaScript in total and are
not part of the Vite production bundle. Chromium installation is separate from
`npm ci`; on the Windows authoring machine the Chromium and headless-shell cache
occupied 731,309,635 bytes (about 697 MiB) outside the repository. The 13
integrated-candidate PNG baselines occupy 774,097 bytes.

A warm integrated-candidate Windows run completed in about 16 seconds. This lane is purposely
not included in `npm run check`: the existing CI matrix is not a Windows pixel
environment, and the browser download is materially larger than the normal
frontend check. A future merge gate should run `npm run test:ui` on pinned
Windows Chromium infrastructure.

## Deliberate limits

- The tracked baselines are Windows Chromium only.
- Fixtures exercise real React surfaces and stores but not Tauri commands,
  native dialogs, WebView destruction/recreation, or signed packages.
- First-launch, Project creation/opening, native error recovery, and a complete
  keyboard-only walkthrough remain W2.8/native acceptance work.
