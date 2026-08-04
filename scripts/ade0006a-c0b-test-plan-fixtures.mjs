import { recordDigest } from "./ade0006a-c0b-common.mjs";
import { buildPairSchedule } from "./validate-ade0006a-c0b-plan.mjs";

const hash = (digit) => digit.repeat(64);

function binding(kind, digit, frozen) {
  const candidate = kind === "candidate";
  return {
    kind,
    sourceCommit: digit.repeat(40),
    sourceTree: hash(digit),
    sourceAggregateSha256: hash(digit),
    sourceManifestSha256: hash(candidate ? "1" : "2"),
    sourceFileCount: candidate ? 500 : 450,
    sourceBytes: candidate ? 4_500_000 : 4_000_000,
    contractManifestSha256: candidate
      ? frozen.contractManifestSha256
      : hash("3"),
    cargoLockSha256: hash(candidate ? "4" : "5"),
    frontendLockSha256: hash(candidate ? "6" : "7"),
    executableSha256: hash(digit),
    executableBytes: candidate ? 20_000_000 : 19_500_000,
    featureSet: candidate ? ["acp-schema"] : ["ade-0005"],
    platform: "windows",
    architecture: "x86_64",
    dependencyGraphSha256: hash(candidate ? "8" : "9"),
    dependencyPackageCount: candidate ? 150 : 145,
    frontendRawBytes: candidate ? 250_000 : 200_000,
    productionPackageSha256: hash(candidate ? "a" : "b"),
    productionPackageBytes: candidate ? 35_000_000 : 34_000_000,
    installerSha256: hash(candidate ? "c" : "d"),
    installerBytes: candidate ? 36_000_000 : 35_000_000,
    installedCoreBytes: candidate ? 31_000_000 : 30_000_000,
  };
}

export function makeTrust(frozen) {
  const provenance = {
    testOwner: "independent-test",
    reviewRole: "independent-reviewer",
    hostClass: "fixed-host-1",
    hardware: "fixed-hardware-1",
    osVersion: "windows-11-24h2",
    platform: "windows",
    architecture: "x86_64",
    powerPolicy: "balanced-ac",
    webViewRuntime: "webview2-150.0.4078.65",
    toolchain: {
      node: "22.18.0",
      rustc: "1.97.1",
      cargo: "1.97.1",
      tauri: "2.8.5",
    },
    collector: { name: "c0b-collector", sha256: hash("1") },
  };
  return {
    schemaVersion: 1,
    workItem: "ADE-0006A",
    phase: "C0-B",
    candidate: binding("candidate", "e", frozen),
    control: binding("control", "f", frozen),
    provenance,
    tools: [
      { name: "c0b-collector", sha256: hash("1") },
      { name: "node", sha256: hash("2") },
    ],
  };
}

export function makePlan(trust, trustSha256, frozen) {
  const run = frozen.budgets.measurementContract.runOrder;
  const sample = frozen.budgets.sampleContract;
  const idle = frozen.budgets.measurementContract.readyIdle;
  const anchors = frozen.budgets.measurementContract.latencyAnchors;
  return {
    schemaVersion: 1,
    mode: "dry-run",
    workItem: "ADE-0006A",
    phase: "C0-B",
    trustManifestSha256: trustSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    fixtureManifestSha256: frozen.fixtureManifestSha256,
    limitsSha256: frozen.limitsSha256,
    candidateBindingSha256: recordDigest(trust.candidate),
    controlBindingSha256: recordDigest(trust.control),
    provenance: structuredClone(trust.provenance),
    collection: {
      validPairs: sample.cleanResourceLaunches,
      excludedWarmups: run.warmupCyclesExcluded,
      percentileMethod: sample.percentileMethod,
      runOrder: run.order,
      launchSamplesPerParticipant: sample.cleanResourceLaunches,
      receiptToPaintSamplesPerParticipant: sample.receiptToPaintSamples,
      inputToPaintSamplesPerParticipant: sample.inputToPaintSamples,
      tabSwitchSamplesPerParticipant: sample.inputToPaintSamples,
      cancelDispatchSamplesPerParticipant: sample.inputToPaintSamples,
      readyIdleSamplesPerParticipant:
        sample.cleanResourceLaunches * idle.samplesPerLaunch,
      saturationCyclesPerBoundary: sample.saturationCyclesPerBoundary,
      fairnessCycles: sample.saturationCyclesPerBoundary,
      coalescingSamplesPerPolicy: sample.receiptToPaintSamples,
      coalescingPolicies: ["16ms", "33ms", "50ms", "4kib"],
    },
    warmupPlan: buildPairSchedule(run.warmupCyclesExcluded).map(
      ({ pairOrdinal, order }) => ({
        warmupOrdinal: pairOrdinal,
        order,
        excluded: true,
      }),
    ),
    anchors: {
      spawnToProcessReady: anchors.spawnToProcessReady,
      initialize: anchors.initialize,
      firstNormalizedOutput: anchors.firstNormalizedOutput,
      nativeReceiptToPaint: anchors.nativeReceiptToPaint,
      inputToPaint: anchors.inputToPaint,
      tabSwitch:
        "trusted tab activation through browser performance.now mark after resulting paint",
      cancelDispatch: anchors.cancelDispatch,
      cancelTerminal: anchors.cancelTerminal,
      coldActivation: "activation receipt through durable ready projection",
      explicitClose: anchors.explicitClose,
      forcedTeardown: "forced termination start through zero owned descendants",
    },
    processMembership: structuredClone(
      frozen.budgets.measurementContract.processMembership,
    ),
    isolation: {
      isolatedData: true,
      normalSageDataAllowed: false,
      defaultPackageArtifactsMutable: false,
      systemOrNetworkControlsAllowed: false,
      installRequired: false,
    },
    commandTemplates: [
      {
        tool: "c0b-collector",
        toolSha256: hash("1"),
        subcommand: "collect",
        fixedFlags: ["--acceptance"],
        placeholderNames: ["<candidate-executable>", "<isolated-data-dir>"],
      },
    ],
  };
}
