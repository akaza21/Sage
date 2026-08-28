import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { withAdeCampaignAdmission } from "./ade0006a-admission.mjs";
import {
  bindC0BCampaignObservationRequest,
  composeC0BCampaignObservationLaunch,
} from "./ade0006a-c0b-campaign-launch.mjs";
import { sha256 } from "./ade0006a-c0b-common.mjs";
import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import { selectPackagedObservationExecutable } from "./ade0006a-c0b-package-reference.mjs";
import { makeC0BFixture } from "./ade0006a-c0b-test-results-fixtures.mjs";

const CAMPAIGN_STATE_PLAN = {
  setupActions: [],
  readySelector: {
    name: "Ready",
    automationId: "",
    controlType: "group",
    view: "control",
  },
  inputProbe: {
    selector: {
      name: "Hide",
      automationId: "",
      controlType: "button",
      view: "control",
    },
    operation: "invoke",
    value: null,
    resultSelector: {
      name: "Show",
      automationId: "",
      controlType: "button",
      view: "control",
    },
    expectedValue: null,
  },
  tabProbe: {
    selector: {
      name: "Tab",
      automationId: "",
      controlType: "tabitem",
      view: "control",
    },
    operation: "select",
    value: null,
    resultSelector: {
      name: "Panel",
      automationId: "",
      controlType: "group",
      view: "control",
    },
    expectedValue: null,
  },
  restoreActions: [],
};

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frozen = loadFrozenContracts(root);
const binderPath = fileURLToPath(
  new URL("./ade0006a-c0b-campaign-launch.mjs", import.meta.url),
);
const observationModulePath = fileURLToPath(
  new URL("./footprint/C0BObservation.psm1", import.meta.url),
);

function makeArtifacts(authenticatedPackage) {
  return Object.fromEntries(
    ["candidate", "control"].map((participant) => {
      const expected = authenticatedPackage.participants[participant];
      return [
        participant,
        {
          installedExecutables: {
            [authenticatedPackage.installerKind]: {
              path: path.resolve(`${participant}-installed.exe`),
              sha256: expected.installedExecutableSha256,
              bytes: expected.installedExecutableBytes,
            },
          },
        },
      ];
    }),
  );
}

function expectedExecutable(authenticatedPackage, artifacts, participant) {
  const artifact =
    artifacts[participant].installedExecutables[
      authenticatedPackage.installerKind
    ];
  return {
    path: artifact.path,
    sha256: artifact.sha256,
    bytes: artifact.bytes,
    bindingSha256:
      authenticatedPackage.participants[participant].executableBindingSha256,
  };
}

async function composeLaunch({
  authenticatedPackage,
  artifacts,
  participant,
  sourceRoot,
  stateId = "shell",
}) {
  plantAdmittedStatePlans(sourceRoot);
  return composeC0BCampaignObservationLaunch({
    sourceRoot,
    frozen,
    authenticatedPackage,
    artifacts,
    participant,
    stateId,
  });
}

test("campaign observation launch binds candidate and control only through the Package selector", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-launch-"));
  try {
    for (const participant of ["candidate", "control"]) {
      const selected = selectPackagedObservationExecutable({
        authenticatedPackage,
        artifacts,
        participant,
      });
      const launch = await composeLaunch({
        authenticatedPackage,
        artifacts,
        participant,
        sourceRoot,
      });
      assert.deepEqual(
        selected,
        expectedExecutable(authenticatedPackage, artifacts, participant),
      );
      assert.deepEqual(launch.executable, selected);
      assert.equal(launch.participant, participant);
      assert.ok(Object.isFrozen(launch));
      assert.ok(Object.isFrozen(launch.executable));
    }
    const ignoredExecutable = {
      path: artifacts.control.installedExecutables[
        authenticatedPackage.installerKind
      ].path,
      sha256: "e".repeat(64),
      bytes:
        authenticatedPackage.participants.candidate.installedExecutableBytes +
        1,
      bindingSha256: "f".repeat(64),
    };
    const launch = await composeC0BCampaignObservationLaunch({
      sourceRoot,
      frozen,
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      executable: ignoredExecutable,
      stateId: "shell",
      statePlan: CAMPAIGN_STATE_PLAN,
    });
    assert.deepEqual(
      launch.executable,
      expectedExecutable(authenticatedPackage, artifacts, "candidate"),
    );
    assert.notDeepEqual(launch.executable, ignoredExecutable);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation launch rejects a control executable in the candidate slot", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  artifacts.candidate.installedExecutables[authenticatedPackage.installerKind] =
    artifacts.control.installedExecutables[authenticatedPackage.installerKind];
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-launch-"));
  try {
    await assert.rejects(
      composeLaunch({
        authenticatedPackage,
        artifacts,
        participant: "candidate",
        sourceRoot,
      }),
      /packaged-observation-executable-mismatch/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation launch rejects SHA and byte swaps", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-launch-"));
  const candidate =
    artifacts.candidate.installedExecutables[
      authenticatedPackage.installerKind
    ];
  try {
    for (const patched of [
      { ...candidate, sha256: "e".repeat(64) },
      {
        ...candidate,
        bytes:
          authenticatedPackage.participants.candidate.installedExecutableBytes +
          1,
      },
    ]) {
      const invalid = structuredClone(artifacts);
      invalid.candidate.installedExecutables[
        authenticatedPackage.installerKind
      ] = patched;
      await assert.rejects(
        composeLaunch({
          authenticatedPackage,
          artifacts: invalid,
          participant: "candidate",
          sourceRoot,
        }),
        /packaged-observation-executable-mismatch/,
      );
    }
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation launch rejects a swapped Package binding digest", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const invalidPackage = structuredClone(authenticatedPackage);
  invalidPackage.participants.candidate.executableBindingSha256 = "f".repeat(
    64,
  );
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-launch-"));
  try {
    await assert.rejects(
      composeLaunch({
        authenticatedPackage: invalidPackage,
        artifacts,
        participant: "candidate",
        sourceRoot,
      }),
      /packaged-observation-executable-mismatch/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

function requestForLaunch(launch) {
  return {
    participant: launch.participant,
    executable: {
      path: launch.executable.path,
      sha256: launch.executable.sha256,
      bytes: launch.executable.bytes,
      bindingSha256: launch.executable.bindingSha256,
    },
    stateId: launch.stateId,
    statePlan: structuredClone(admittedStatePlan(launch.stateId)),
  };
}

test("campaign observation request bind accepts matching candidate and control launches", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-"));
  try {
    for (const participant of ["candidate", "control"]) {
      const launch = await composeLaunch({
        authenticatedPackage,
        artifacts,
        participant,
        sourceRoot,
      });
      const request = requestForLaunch(launch);
      const bound = bindC0BCampaignObservationRequest({ request, launch });
      assert.equal(bound, request);
      assert.equal(bound.participant, participant);
      assert.deepEqual(bound.executable, launch.executable);
      assert.ok(Object.isFrozen(bound));
    }
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation request bind rejects SHA, byte, participant, binding, and control swaps", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(
    path.join(tmpdir(), "sage-c0b-bind-mismatch-"),
  );
  try {
    const candidateLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    const controlLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "control",
      sourceRoot,
    });
    const mismatches = [
      {
        ...requestForLaunch(candidateLaunch),
        executable: {
          ...candidateLaunch.executable,
          sha256: controlLaunch.executable.sha256,
        },
      },
      {
        ...requestForLaunch(candidateLaunch),
        executable: {
          ...candidateLaunch.executable,
          bytes: candidateLaunch.executable.bytes + 1,
        },
      },
      {
        participant: "candidate",
        executable: {
          path: controlLaunch.executable.path,
          sha256: controlLaunch.executable.sha256,
          bytes: controlLaunch.executable.bytes,
          bindingSha256: controlLaunch.executable.bindingSha256,
        },
      },
      {
        ...requestForLaunch(candidateLaunch),
        participant: "control",
      },
      {
        ...requestForLaunch(candidateLaunch),
        executable: {
          ...candidateLaunch.executable,
          bindingSha256: controlLaunch.executable.bindingSha256,
        },
      },
    ];
    for (const request of mismatches) {
      const original = structuredClone(request);
      assert.throws(
        () =>
          bindC0BCampaignObservationRequest({
            request,
            launch: candidateLaunch,
          }),
        /packaged-observation-executable-mismatch/,
      );
      assert.deepEqual(request, original);
    }
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation launch pins a state-plan digest and bind accepts that plan", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-plan-"));
  try {
    const launch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    assert.equal(launch.statePlanSha256, independentStatePlanSha256("shell"));
    assert.equal("statePlan" in launch, false);
    const request = requestForLaunch(launch);
    const bound = bindC0BCampaignObservationRequest({ request, launch });
    assert.equal(bound, request);
    assert.deepEqual(bound.statePlan, admittedStatePlan("shell"));
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation request bind rejects a swapped inputProbe after compose", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(
    path.join(tmpdir(), "sage-c0b-bind-plan-swap-"),
  );
  try {
    const launch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    const request = requestForLaunch(launch);
    request.statePlan.inputProbe.operation = "select";
    const original = structuredClone(request);
    assert.throws(
      () => bindC0BCampaignObservationRequest({ request, launch }),
      /campaign-state-plan-mismatch/,
    );
    assert.deepEqual(request, original);
    assert.equal(launch.statePlanSha256, independentStatePlanSha256("shell"));
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation request bind rejects a matching executable with a missing state plan", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(
    path.join(tmpdir(), "sage-c0b-bind-plan-missing-"),
  );
  try {
    const launch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    const request = requestForLaunch(launch);
    delete request.statePlan;
    assert.throws(
      () => bindC0BCampaignObservationRequest({ request, launch }),
      /campaign-state-plan-invalid/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

function plantAdmittedStatePlans(sourceRoot) {
  const relative = ["scripts", "c0b-state-plans"];
  mkdirSync(path.join(sourceRoot, ...relative), { recursive: true });
  for (const stateId of frozen.budgets.sampleContract.packagedStates) {
    copyFileSync(
      path.join(root, ...relative, `${stateId}.json`),
      path.join(sourceRoot, ...relative, `${stateId}.json`),
    );
  }
}

function admittedStatePlan(stateId) {
  return JSON.parse(
    readFileSync(
      path.join(root, "scripts", "c0b-state-plans", `${stateId}.json`),
      "utf8",
    ),
  );
}

function independentStatePlanSha256(stateId) {
  return sha256(JSON.stringify(admittedStatePlan(stateId)));
}

test("admitted state plans use footprint-control accessible names", () => {
  const html = readFileSync(
    path.join(root, "tools", "footprint-control", "frontend", "index.html"),
    "utf8",
  );
  const navNames = Object.fromEntries(
    [...html.matchAll(/data-state="([^"]+)">([^<]+)</g)].map(([, id, name]) => [
      id,
      name.trim(),
    ]),
  );
  const markup = readFileSync(
    path.join(root, "tools", "footprint-control", "frontend", "main.js"),
    "utf8",
  );
  assert.match(markup, /Footprint control \$\{state\} state/);
  assert.match(markup, /Hide right sidebar/);
  assert.match(markup, />Control secondary tab</);
  assert.match(markup, /Control secondary panel/);

  for (const stateId of frozen.budgets.sampleContract.packagedStates) {
    const plan = admittedStatePlan(stateId);
    assert.equal(plan.setupActions[0].selector.name, navNames[stateId]);
    assert.equal(plan.setupActions[0].operation, "toggle");
    assert.equal(plan.readySelector.name, `Footprint control ${stateId} state`);
    assert.equal(plan.inputProbe.selector.name, "Hide right sidebar");
    assert.equal(plan.inputProbe.resultSelector.name, "Show right sidebar");
    assert.equal(plan.tabProbe.selector.name, "Control secondary tab");
    assert.equal(plan.tabProbe.resultSelector.name, "Control secondary panel");
  }
});

test("campaign observation launch hashes the admitted state-id plan from disk", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(
    path.join(tmpdir(), "sage-c0b-state-catalog-"),
  );
  try {
    plantAdmittedStatePlans(sourceRoot);
    const mutated = structuredClone(admittedStatePlan("shell"));
    mutated.inputProbe.operation = "select";
    const launch = await composeC0BCampaignObservationLaunch({
      sourceRoot,
      frozen,
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      stateId: "shell",
      statePlan: mutated,
    });
    assert.equal(launch.stateId, "shell");
    assert.equal(launch.statePlanSha256, independentStatePlanSha256("shell"));
    assert.notEqual(launch.statePlanSha256, sha256(JSON.stringify(mutated)));
    assert.equal("statePlan" in launch, false);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation launch rejects an unadmitted state id", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(
    path.join(tmpdir(), "sage-c0b-state-unknown-"),
  );
  try {
    plantAdmittedStatePlans(sourceRoot);
    await assert.rejects(
      composeC0BCampaignObservationLaunch({
        sourceRoot,
        frozen,
        authenticatedPackage,
        artifacts,
        participant: "candidate",
        stateId: "review",
      }),
      /campaign-state-plan-unadmitted/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation bind rejects a chat plan on a shell launch", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-state-swap-"));
  try {
    const shellLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
      stateId: "shell",
    });
    const chatLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
      stateId: "chat",
    });
    assert.notEqual(shellLaunch.statePlanSha256, chatLaunch.statePlanSha256);
    const request = requestForLaunch(chatLaunch);
    request.stateId = "shell";
    assert.throws(
      () =>
        bindC0BCampaignObservationRequest({
          request,
          launch: shellLaunch,
        }),
      /campaign-state-plan-mismatch/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

function writeBindFiles(directory, request, launch) {
  const requestPath = path.join(directory, "request.json");
  const launchPath = path.join(directory, "launch.json");
  writeFileSync(requestPath, JSON.stringify(request));
  writeFileSync(launchPath, JSON.stringify(launch));
  return { requestPath, launchPath };
}

function runBindCli(requestPath, launchPath) {
  return spawnSync(
    process.execPath,
    [binderPath, "bind", requestPath, launchPath],
    { encoding: "utf8" },
  );
}

function runPlanCli(stateId) {
  return spawnSync(process.execPath, [binderPath, "plan", stateId], {
    encoding: "utf8",
    cwd: root,
  });
}

test("campaign observation plan CLI prints the admitted plan from the candidate checkout", () => {
  const result = runPlanCli("shell");
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), admittedStatePlan("shell"));
});

test("campaign observation plan CLI rejects an unadmitted state id", () => {
  const result = runPlanCli("review");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /campaign-state-plan-unadmitted/);
});

test("control smoke loads the admitted shell plan through the plan CLI", () => {
  const smoke = readFileSync(
    path.join(root, "scripts", "test-c0b-control-smoke.ps1"),
    "utf8",
  );
  assert.match(smoke, /\bplan\b.*\bshell\b/);
  assert.doesNotMatch(smoke, /New-C0BAction/);
  assert.doesNotMatch(smoke, /New-C0BSelector/);
});

test("campaign observation bind CLI accepts a matching request without starting a process", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-cli-"));
  try {
    const launch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "control",
      sourceRoot,
    });
    assert.equal(existsSync(launch.executable.path), false);
    const { requestPath, launchPath } = writeBindFiles(
      sourceRoot,
      requestForLaunch(launch),
      launch,
    );
    const result = runBindCli(requestPath, launchPath);
    assert.equal(result.status, 0, result.stderr);
    const bound = JSON.parse(result.stdout);
    assert.equal(bound.participant, "control");
    assert.deepEqual(bound.executable, launch.executable);
    assert.equal(existsSync(launch.executable.path), false);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation bind CLI rejects a SHA-swapped request", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-cli-bad-"));
  try {
    const candidateLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    const controlLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "control",
      sourceRoot,
    });
    const request = requestForLaunch(candidateLaunch);
    request.executable.sha256 = controlLaunch.executable.sha256;
    const { requestPath, launchPath } = writeBindFiles(
      sourceRoot,
      request,
      candidateLaunch,
    );
    const result = runBindCli(requestPath, launchPath);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /packaged-observation-executable-mismatch/);
    assert.equal(existsSync(candidateLaunch.executable.path), false);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign collector bind rejects a mismatched request before starting a process", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-ps-"));
  try {
    const candidateLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    const controlLaunch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "control",
      sourceRoot,
    });
    const request = requestForLaunch(candidateLaunch);
    request.executable.sha256 = controlLaunch.executable.sha256;
    const { requestPath, launchPath } = writeBindFiles(
      sourceRoot,
      request,
      candidateLaunch,
    );
    const scriptPath = path.join(sourceRoot, "bind-campaign.ps1");
    const quote = (value) => `'${String(value).replace(/'/g, "''")}'`;
    writeFileSync(
      scriptPath,
      [
        "Set-StrictMode -Version Latest",
        "$ErrorActionPreference = 'Stop'",
        `Import-Module -Name ${quote(observationModulePath)} -Force`,
        `Invoke-C0BCampaignObservationRequest -RequestPath ${quote(requestPath)} -LaunchPath ${quote(launchPath)}`,
      ].join("\r\n"),
    );
    const result = spawnSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-File", scriptPath],
      { encoding: "utf8" },
    );
    assert.notEqual(result.status, 0);
    assert.match(
      `${result.stdout}\n${result.stderr}`,
      /packaged-observation-executable-mismatch/,
    );
    assert.doesNotMatch(
      `${result.stdout}\n${result.stderr}`,
      /Observation executable identity is invalid/,
    );
    assert.equal(existsSync(candidateLaunch.executable.path), false);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("campaign observation request bind does not start a process", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-bind-pure-"));
  try {
    const launch = await composeLaunch({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
      sourceRoot,
    });
    assert.equal(existsSync(launch.executable.path), false);
    const request = requestForLaunch(launch);
    const bound = bindC0BCampaignObservationRequest({ request, launch });
    assert.equal(bound, request);
    assert.equal(existsSync(launch.executable.path), false);
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

test("a held Package admission lock rejects C0-B campaign launch composition", async () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = makeArtifacts(authenticatedPackage);
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-launch-"));
  try {
    await withAdeCampaignAdmission({
      sourceRoot,
      frozen,
      phase: "Package",
      action: async () => {
        await assert.rejects(
          composeLaunch({
            authenticatedPackage,
            artifacts,
            participant: "candidate",
            sourceRoot,
          }),
          /capacity-rejected/,
        );
      },
    });
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});
