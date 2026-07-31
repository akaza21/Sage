import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(scriptDirectory, "..");
const defaultBudgetPath = path.join(
  workspaceRoot,
  "docs",
  "performance",
  "budgets.json",
);
const summaryPaths = new Map([
  [
    "ADE-0004",
    path.join(
      workspaceRoot,
      "docs",
      "work-items",
      "ADE-0004-windows-idle-footprint",
      "evidence",
      "curated-summary.json",
    ),
  ],
  [
    "ADE-0005",
    path.join(
      workspaceRoot,
      "docs",
      "work-items",
      "ADE-0005-project-lifecycle",
      "evidence",
      "curated-summary.json",
    ),
  ],
]);
const defaultWorkItem = "ADE-0005";
const tolerance = 0.002;

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function addLimitCheck(errors, checks, label, actual, limit) {
  checks.push({ label, actual, limit });
  if (!Number.isFinite(actual) || actual > limit + tolerance) {
    errors.push(`${label}: ${actual} exceeds ${limit}`);
  }
}

function assertStateSet(errors, actualStates, expectedStates, className) {
  const actual = [...actualStates].sort().join(",");
  const expected = [...expectedStates].sort().join(",");
  if (actual !== expected) {
    errors.push(`${className} states: expected ${expected}, got ${actual}`);
  }
}

export function validatePerformanceBudgets(budgets, summary) {
  const errors = [];
  const checks = [];
  const windows = budgets.platformRuntimeOverrides?.windowsWebView2;
  if (!windows) {
    return { errors: ["windowsWebView2 override is missing"], checks };
  }
  if (summary.sanitized !== true) {
    errors.push("curated summary must declare sanitized=true");
  }
  if (
    !summary.runtime?.chat &&
    summary.measurement?.webView2Version !== windows.observedRuntimeVersion
  ) {
    errors.push(
      `WebView2 version mismatch: expected ${windows.observedRuntimeVersion}, ` +
        `got ${summary.measurement?.webView2Version}`,
    );
  }
  const requiredSupersededGates = [
    "runtime.idleWorkbenchRssP95Mb",
    "runtime.activeLargeChatRssP95Mb",
  ];
  for (const gate of requiredSupersededGates) {
    if (!windows.supersedes?.includes(gate)) {
      errors.push(`Windows override must supersede ${gate}`);
    }
  }
  if (
    windows.relativeMedianDefinition !==
    "state_private_median_mb - control_private_median_mb"
  ) {
    errors.push("relative median definition is missing or ambiguous");
  }
  if (
    windows.relativeP95Definition !==
    "state_private_p95_mb - control_private_p95_mb"
  ) {
    errors.push("relative P95 definition is missing or ambiguous");
  }

  if (summary.runtime?.chat) {
    return validateAde0005Budgets(budgets, summary, errors, checks, windows);
  }

  const control = summary.control?.privateMiB;
  if (!control) {
    return {
      errors: [...errors, "control private-byte summary is missing"],
      checks,
    };
  }
  if (
    Math.abs(control.median - windows.controlFloor.medianPrivateMb) >
      tolerance ||
    Math.abs(control.p95 - windows.controlFloor.p95PrivateMb) > tolerance
  ) {
    errors.push("curated control floor does not match the budget baseline");
  }
  addLimitCheck(
    errors,
    checks,
    "control private P95 MiB",
    control.p95,
    windows.controlFloor.p95LimitMb,
  );

  const states = summary.states ?? [];
  for (const className of ["idle", "active"]) {
    const contract = windows[className];
    const classStates = states.filter((state) => state.class === className);
    assertStateSet(
      errors,
      classStates.map((state) => state.name),
      contract.states,
      className,
    );
    for (const state of classStates) {
      const derivedMedian = state.privateMiB.median - control.median;
      const derivedP95 = state.privateMiB.p95 - control.p95;
      if (
        Math.abs(derivedMedian - state.overControlMiB.median) > tolerance ||
        Math.abs(derivedP95 - state.overControlMiB.p95) > tolerance
      ) {
        errors.push(
          `${state.name}: curated control-relative values are inconsistent`,
        );
      }
      addLimitCheck(
        errors,
        checks,
        `${state.name} absolute private P95 MiB`,
        state.privateMiB.p95,
        contract.absolutePrivateP95Mb,
      );
      addLimitCheck(
        errors,
        checks,
        `${state.name} over-control private median MiB`,
        derivedMedian,
        contract.overControlPrivateMedianMb,
      );
      addLimitCheck(
        errors,
        checks,
        `${state.name} over-control private P95 MiB`,
        derivedP95,
        contract.overControlPrivateP95Mb,
      );
      addLimitCheck(
        errors,
        checks,
        `${state.name} idle CPU P95 percent`,
        state.idleCpuP95Percent,
        budgets.runtime.idleCpuSingleCorePercent,
      );
      addLimitCheck(
        errors,
        checks,
        `${state.name} startup P95 ms`,
        state.startupWindowP95Ms,
        budgets.runtime.warmWindowP95Ms,
      );
    }
  }

  const chatInteraction = summary.interaction?.chat;
  addLimitCheck(
    errors,
    checks,
    "chat input-to-paint P95 ms",
    chatInteraction?.inputToPaintMs?.p95,
    budgets.runtime.inputToPaintP95Ms,
  );
  addLimitCheck(
    errors,
    checks,
    "chat tab-switch P95 ms",
    chatInteraction?.tabSwitchMs?.p95,
    budgets.runtime.tabSwitchP95Ms,
  );

  const finalArtifacts = summary.artifacts?.final;
  addLimitCheck(
    errors,
    checks,
    "Windows installer MiB",
    finalArtifacts?.installerBytes / 1024 / 1024,
    budgets.bundle.windowsInstallerMb,
  );
  addLimitCheck(
    errors,
    checks,
    "installed core MiB",
    finalArtifacts?.executableBytes / 1024 / 1024,
    budgets.bundle.installedCoreMb,
  );

  return { errors, checks };
}

function validateAde0005Budgets(budgets, summary, errors, checks, windows) {
  if (summary.workItem !== "ADE-0005") {
    errors.push("active summary must declare workItem=ADE-0005");
  }
  if (summary.runtime.webView2Version !== windows.observedRuntimeVersion) {
    errors.push(
      `WebView2 version mismatch: expected ${windows.observedRuntimeVersion}, ` +
        `got ${summary.runtime.webView2Version}`,
    );
  }
  const bytesPerMiB = 1024 * 1024;
  for (const name of ["chat", "explorer"]) {
    const state = summary.runtime[name];
    const median = state?.privateMedianBytes / bytesPerMiB;
    const p95 = state?.privateP95Bytes / bytesPerMiB;
    addLimitCheck(
      errors,
      checks,
      `${name} absolute private P95 MiB`,
      p95,
      windows.idle.absolutePrivateP95Mb,
    );
    addLimitCheck(
      errors,
      checks,
      `${name} over-control private median MiB`,
      median - windows.controlFloor.medianPrivateMb,
      windows.idle.overControlPrivateMedianMb,
    );
    addLimitCheck(
      errors,
      checks,
      `${name} over-control private P95 MiB`,
      p95 - windows.controlFloor.p95PrivateMb,
      windows.idle.overControlPrivateP95Mb,
    );
    addLimitCheck(
      errors,
      checks,
      `${name} idle CPU P95 percent`,
      state?.idleCpuP95Percent,
      budgets.runtime.idleCpuSingleCorePercent,
    );
  }
  addLimitCheck(
    errors,
    checks,
    "chat input-to-paint P95 ms",
    summary.runtime.chat.inputToPaintP95Ms,
    budgets.runtime.inputToPaintP95Ms,
  );
  addLimitCheck(
    errors,
    checks,
    "chat tab-switch P95 ms",
    summary.runtime.chat.tabSwitchP95Ms,
    budgets.runtime.tabSwitchP95Ms,
  );
  addLimitCheck(
    errors,
    checks,
    "Windows installer MiB",
    summary.artifacts?.final?.nsisBytes / bytesPerMiB,
    budgets.bundle.windowsInstallerMb,
  );
  addLimitCheck(
    errors,
    checks,
    "installed core MiB",
    summary.artifacts?.final?.executableBytes / bytesPerMiB,
    budgets.bundle.installedCoreMb,
  );
  return { errors, checks };
}

export function validateFiles(
  budgetPath = defaultBudgetPath,
  summaryPath = summaryPaths.get(defaultWorkItem),
) {
  return validatePerformanceBudgets(
    readJson(budgetPath),
    readJson(summaryPath),
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const requestedWorkItem = process.argv[2] ?? defaultWorkItem;
  const summaryPath = summaryPaths.get(requestedWorkItem);
  if (!summaryPath) {
    console.error(
      `budget: unknown work item ${requestedWorkItem}; expected ${[...summaryPaths.keys()].join(" or ")}`,
    );
    process.exitCode = 1;
  }
  const result = summaryPath
    ? validateFiles(defaultBudgetPath, summaryPath)
    : { errors: ["summary selection failed"], checks: [] };
  if (result.errors.length > 0) {
    for (const error of result.errors) {
      console.error(`budget: ${error}`);
    }
    process.exitCode = 1;
  } else {
    console.log(
      JSON.stringify({
        status: "passed",
        checks: result.checks.length,
        workItem: requestedWorkItem,
        summary: `%WORKSPACE%/${path.relative(workspaceRoot, summaryPath).replaceAll(path.sep, "/")}`,
      }),
    );
  }
}
