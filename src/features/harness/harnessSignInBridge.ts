import { useEffect, useState } from "react";

import { isTauriRuntime } from "../../ports/desktopRuntime";
import type { HarnessSignInDecoration } from "./harnessSignIn";

/**
 * Keeps the harness sign-in module out of the initial Agent bundle: it loads
 * only in the desktop runtime, when the picker has harnesses to decorate.
 */
export function useHarnessSignInDecorations(
  harnessDefinitions: readonly {
    id: string;
    name: string;
    installState: string;
  }[],
  enabled = isTauriRuntime(),
): ReadonlyMap<string, HarnessSignInDecoration> {
  const harnesses = harnessDefinitions.filter(
    (harness) => harness.installState === "available",
  );
  const [decorations, setDecorations] = useState<
    ReadonlyMap<string, HarnessSignInDecoration>
  >(() => new Map());
  const key = harnesses
    .map((harness) => `${harness.id}\0${harness.name}`)
    .join("\n");
  useEffect(() => {
    if (!enabled || !key) return;
    let active = true;
    let unsubscribe: (() => void) | undefined;
    const entries = key.split("\n").map((entry) => {
      const [id, name] = entry.split("\0");
      return { id, name };
    });
    void import("./harnessSignIn").then((signIn) => {
      if (!active) return;
      const publish = () => {
        if (active) setDecorations(signIn.harnessSignInDecorations(entries));
      };
      unsubscribe = signIn.subscribeHarnessSignIn(publish);
      publish();
      signIn.ensureHarnessSignInChecked(entries.map((entry) => entry.id));
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [enabled, key]);
  return decorations;
}

/** Adds a harness's sign-in label and action to its picker row. */
export function withSignIn<
  Candidate extends { id: string; accessibleLabel?: string },
>(
  candidate: Candidate,
  modelSubtitle: string | undefined,
  decorations: ReadonlyMap<string, HarnessSignInDecoration>,
) {
  const decoration = decorations.get(candidate.id);
  const signInLabel = decoration?.label ?? null;
  return {
    ...candidate,
    subtitle:
      [modelSubtitle, signInLabel].filter(Boolean).join(" · ") || undefined,
    accessibleLabel: signInLabel
      ? `${candidate.accessibleLabel}. ${signInLabel}`
      : candidate.accessibleLabel,
    action: decoration?.action,
  };
}
