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

The lane runs 27 tests serially with a fixed clock, locale, timezone, font wait,
disabled screenshot animations, hidden carets, and explicit transcript
follow-end settlement. Screenshots are scoped to `main.workbench`; role
assertions verify important landmarks and controls before capture. Every
responsive case also rejects document-level horizontal overflow.

## Coverage

| State | Browser proof |
| --- | --- |
| Agent | Single Chat, dual Chat, and a bounded 14-turn transcript |
| Editor | Explorer, the functional welcome surface, populated/empty Search, Source Control plus loading/error and complete control-interaction states, populated/empty Problems and Outline, tabs, breadcrumbs, CodeMirror, dirty/save status, and status bar |
| Resources | Agent Diff, loopback Preview, and empty right pane |
| Recovery | Failed Turn and waiting/running Turn |
| Accessibility | Forced colors, reduced motion, and keyboard focus-visible |
| Responsive | 720x560 narrow window and a 1380x880 device emulated at 200% zoom |

The zoom case uses Chromium device metrics with a 2x scale factor and asserts
both a 690 CSS-pixel effective width and `devicePixelRatio === 2` before capture.
It therefore protects the same reflow boundary as a 1380x880 window at 200%,
while the native walkthrough remains responsible for WebView integration.

## Dependency and runtime impact

`@playwright/test` 1.62.1 is an exact, dev-only dependency. Its three installed
npm packages have approximately 18.5 MB of unpacked JavaScript in total and are
not part of the Vite production bundle. Chromium installation is separate from
`npm ci`; on the Windows authoring machine the Chromium and headless-shell cache
occupied 731,309,635 bytes (about 697 MiB) outside the repository. The 27
integrated-candidate PNG baselines occupy 1,449,938 bytes.

A warm integrated-candidate Windows run completed in about 22 seconds. This
lane is purposely not included in `npm run check`: the existing CI matrix is
not a Windows pixel environment, and the browser download is materially larger
than the normal frontend check. A future merge gate should run
`npm run test:ui` on pinned Windows Chromium infrastructure.

## Deliberate limits

- The tracked baselines are Windows Chromium only.
- Fixtures exercise real React surfaces and stores but not Tauri commands,
  native dialogs, WebView destruction/recreation, or signed packages.
- Signed-package, macOS, and operating-system assistive-technology acceptance
  remain outside this browser lane.
