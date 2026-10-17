import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_SESSION_ACCESS_MODE,
  type SessionAccessModes,
} from "../domain/sessionAccessMode";
import { DEFAULT_UI_APPEARANCE } from "../domain/uiAppearance";
import {
  getWorkbenchPorts,
  resetWorkbenchPortsForTests,
} from "../ports/workbenchRuntime";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import { useUiAppearanceStore } from "./uiAppearanceStore";

beforeEach(async () => {
  vi.restoreAllMocks();
  resetWorkbenchPortsForTests();
  await getPorts().settings.setSessionAccessModes({
    "binding-test": DEFAULT_SESSION_ACCESS_MODE,
  });
  await getPorts().settings.setUiAppearance(DEFAULT_UI_APPEARANCE);
  useSessionAccessModeStore.setState({
    modes: { "binding-test": DEFAULT_SESSION_ACCESS_MODE },
    ready: false,
    error: null,
  });
  useUiAppearanceStore.setState({
    appearance: DEFAULT_UI_APPEARANCE,
    ready: false,
    error: null,
  });
});

describe("session access mode store", () => {
  it("starts fail-closed and unready", () => {
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "readOnly",
    );
    expect(useSessionAccessModeStore.getState().ready).toBe(false);
  });

  it("restores the persisted mode through the settings port", async () => {
    await getPorts().settings.setSessionAccessModes({
      "binding-test": "workspaceWrite",
    });
    await useSessionAccessModeStore.getState().restore();
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "workspaceWrite",
    );
    expect(useSessionAccessModeStore.getState().ready).toBe(true);
    expect(useSessionAccessModeStore.getState().error).toBeNull();
  });

  it("ignores writes before the store is ready", async () => {
    const save = vi.spyOn(getPorts().settings, "setSessionAccessModes");
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    expect(save).not.toHaveBeenCalled();
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "readOnly",
    );
  });

  it("writes through and keeps the optimistic value", async () => {
    await useSessionAccessModeStore.getState().restore();
    const save = vi.spyOn(getPorts().settings, "setSessionAccessModes");
    useSessionAccessModeStore.getState().setMode("binding-test", "ask");
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "ask",
    );
    expect(save).toHaveBeenCalledWith({ "binding-test": "ask" });
    await useSessionAccessModeStore.getState().settle();
    expect(await getPorts().settings.getSessionAccessModes()).toEqual({
      "binding-test": "ask",
    });
  });

  it("degrades an unknown persisted value instead of throwing", async () => {
    vi.spyOn(getPorts().settings, "getSessionAccessModes").mockResolvedValue(
      "elevated" as never,
    );
    await useSessionAccessModeStore.getState().restore();
    expect(useSessionAccessModeStore.getState().modes).toEqual({});
    expect(useSessionAccessModeStore.getState().ready).toBe(true);
  });

  it("surfaces a load failure without claiming readiness", async () => {
    vi.spyOn(getPorts().settings, "getSessionAccessModes").mockRejectedValue(
      new Error("nope"),
    );
    await useSessionAccessModeStore.getState().restore();
    expect(useSessionAccessModeStore.getState().ready).toBe(false);
    expect(useSessionAccessModeStore.getState().error).toBe(
      "Permission mode could not be loaded.",
    );
  });

  it("surfaces a save failure and reverts to the persisted value", async () => {
    await useSessionAccessModeStore.getState().restore();
    vi.spyOn(getPorts().settings, "setSessionAccessModes").mockRejectedValue(
      new Error("nope"),
    );
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    await useSessionAccessModeStore.getState().settle();
    expect(useSessionAccessModeStore.getState().error).toBe(
      "Permission mode could not be saved.",
    );
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "readOnly",
    );
  });

  it("keeps the newest persisted choice when an older queued write fails", async () => {
    await useSessionAccessModeStore.getState().restore();
    failNthSessionAccessModeWrite(1);

    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    useSessionAccessModeStore.getState().setMode("binding-test", "ask");
    await useSessionAccessModeStore.getState().settle();

    expect(await getPorts().settings.getSessionAccessModes()).toEqual({
      "binding-test": "ask",
    });
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "ask",
    );
    expect(useSessionAccessModeStore.getState().error).toBeNull();
  });

  it("reverts a failed newest write to the last persisted value", async () => {
    await useSessionAccessModeStore.getState().restore();
    failNthSessionAccessModeWrite(1, 2);

    useSessionAccessModeStore.getState().setMode("binding-test", "ask");
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    await useSessionAccessModeStore.getState().settle();

    expect(await getPorts().settings.getSessionAccessModes()).toEqual({
      "binding-test": "readOnly",
    });
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "readOnly",
    );
    expect(useSessionAccessModeStore.getState().error).toBe(
      "Permission mode could not be saved.",
    );
  });

  it("coalesces rapid choices into a bounded number of native writes", async () => {
    await useSessionAccessModeStore.getState().restore();
    const save = vi.spyOn(getPorts().settings, "setSessionAccessModes");

    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    useSessionAccessModeStore.getState().setMode("binding-test", "readOnly");
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");
    useSessionAccessModeStore.getState().setMode("binding-test", "readOnly");
    useSessionAccessModeStore.getState().setMode("binding-test", "ask");
    await useSessionAccessModeStore.getState().settle();

    expect(save.mock.calls.length).toBeLessThanOrEqual(2);
    expect(save).toHaveBeenLastCalledWith({ "binding-test": "ask" });
    expect(await getPorts().settings.getSessionAccessModes()).toEqual({
      "binding-test": "ask",
    });
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "ask",
    );
  });

  it("settles true once the newest choice is persisted", async () => {
    await useSessionAccessModeStore.getState().restore();
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");

    await expect(useSessionAccessModeStore.getState().settle()).resolves.toBe(
      true,
    );
  });

  it("settles false when the newest choice failed to persist", async () => {
    await useSessionAccessModeStore.getState().restore();
    failNthSessionAccessModeWrite(1);
    useSessionAccessModeStore
      .getState()
      .setMode("binding-test", "workspaceWrite");

    await expect(useSessionAccessModeStore.getState().settle()).resolves.toBe(
      false,
    );
  });

  it("settles only after an in-flight restore finishes", async () => {
    const load = deferred<SessionAccessModes>();
    vi.spyOn(getPorts().settings, "getSessionAccessModes").mockReturnValue(
      load.promise,
    );
    void useSessionAccessModeStore.getState().restore();

    let resolved = false;
    const settling = useSessionAccessModeStore
      .getState()
      .settle()
      .then((value) => {
        resolved = true;
        return value;
      });
    await Promise.resolve();
    expect(resolved).toBe(false);

    load.resolve({ "binding-test": "workspaceWrite" });
    await expect(settling).resolves.toBe(true);
    expect(useSessionAccessModeStore.getState().modes["binding-test"]).toBe(
      "workspaceWrite",
    );
  });

  it("settles false when the restore failed", async () => {
    vi.spyOn(getPorts().settings, "getSessionAccessModes").mockRejectedValue(
      new Error("nope"),
    );
    await useSessionAccessModeStore.getState().restore();

    await expect(useSessionAccessModeStore.getState().settle()).resolves.toBe(
      false,
    );
  });
});

describe("ui appearance store", () => {
  it("starts at the reference defaults", () => {
    expect(useUiAppearanceStore.getState().appearance).toEqual({
      fontSize: 14,
      density: "comfortable",
    });
  });

  it("restores and writes through the settings port", async () => {
    await useUiAppearanceStore.getState().restore();
    expect(useUiAppearanceStore.getState().ready).toBe(true);
    useUiAppearanceStore
      .getState()
      .setAppearance({ fontSize: 16, density: "compact" });
    expect(useUiAppearanceStore.getState().appearance).toEqual({
      fontSize: 16,
      density: "compact",
    });
    await useUiAppearanceStore.getState().flush();
    expect(await getPorts().settings.getUiAppearance()).toEqual({
      fontSize: 16,
      density: "compact",
    });
  });

  it("admits an out-of-ramp persisted value to the default", async () => {
    vi.spyOn(getPorts().settings, "getUiAppearance").mockResolvedValue({
      fontSize: 11,
      density: "roomy",
    } as never);
    await useUiAppearanceStore.getState().restore();
    expect(useUiAppearanceStore.getState().appearance).toEqual(
      DEFAULT_UI_APPEARANCE,
    );
  });

  it("keeps the newest persisted appearance when an older queued write fails", async () => {
    await useUiAppearanceStore.getState().restore();
    failNthUiAppearanceWrite(1);

    useUiAppearanceStore
      .getState()
      .setAppearance({ fontSize: 12, density: "compact" });
    useUiAppearanceStore
      .getState()
      .setAppearance({ fontSize: 16, density: "comfortable" });
    await useUiAppearanceStore.getState().flush();

    expect(await getPorts().settings.getUiAppearance()).toEqual({
      fontSize: 16,
      density: "comfortable",
    });
    expect(useUiAppearanceStore.getState().appearance).toEqual({
      fontSize: 16,
      density: "comfortable",
    });
    expect(useUiAppearanceStore.getState().error).toBeNull();
  });

  it("reverts a failed newest appearance write to the last persisted value", async () => {
    await useUiAppearanceStore.getState().restore();
    failNthUiAppearanceWrite(1, 2);

    useUiAppearanceStore
      .getState()
      .setAppearance({ fontSize: 12, density: "compact" });
    useUiAppearanceStore
      .getState()
      .setAppearance({ fontSize: 16, density: "comfortable" });
    await useUiAppearanceStore.getState().flush();

    expect(await getPorts().settings.getUiAppearance()).toEqual(
      DEFAULT_UI_APPEARANCE,
    );
    expect(useUiAppearanceStore.getState().appearance).toEqual(
      DEFAULT_UI_APPEARANCE,
    );
    expect(useUiAppearanceStore.getState().error).toBe(
      "Appearance settings could not be saved.",
    );
  });
});

function getPorts() {
  return getWorkbenchPorts();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

function failNthSessionAccessModeWrite(...failed: number[]) {
  const settings = getPorts().settings;
  const write = settings.setSessionAccessModes.bind(settings);
  let call = 0;
  vi.spyOn(settings, "setSessionAccessModes").mockImplementation((mode) => {
    call += 1;
    return failed.includes(call)
      ? Promise.reject(new Error("native write failed"))
      : write(mode);
  });
}

function failNthUiAppearanceWrite(...failed: number[]) {
  const settings = getPorts().settings;
  const write = settings.setUiAppearance.bind(settings);
  let call = 0;
  vi.spyOn(settings, "setUiAppearance").mockImplementation((appearance) => {
    call += 1;
    return failed.includes(call)
      ? Promise.reject(new Error("native write failed"))
      : write(appearance);
  });
}
