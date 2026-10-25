import { create } from "zustand";

import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { sessionConfigControls } from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { composerOptionLabel } from "./composerChrome";

/**
 * The model and effort a side chat or fork inherits from its parent, shown in
 * its composer until its own session reports a configuration (Codex
 * s2/f0244, s2/f0066). Only values the parent's harness confirmed are kept.
 */
export interface InheritedComposerModel {
  model: string;
  effort: string | null;
  /** The confirmed model's id, when the harness reported one. */
  id?: string;
}

/** Side chats are capped at eight; forks are short-lived until first send. */
const MAX_INHERITED = 16;

const useInheritedStore = create<
  Readonly<Record<string, InheritedComposerModel>>
>(() => ({}));

const category = (option: AcpConfigOption) =>
  (option.category ?? option.id).toLocaleLowerCase();

/** The parent's confirmed model and effort labels, or null when unconfirmed. */
export function confirmedComposerModel(
  config: AcpSessionConfig | null | undefined,
): InheritedComposerModel | null {
  if (!config) return null;
  const options = sessionConfigControls(config);
  const modelOption = options.find((option) => category(option) === "model");
  const model =
    composerOptionLabel(modelOption) ??
    config.confirmedModel?.name ??
    config.confirmedModel?.id ??
    null;
  if (!model) return null;
  const effortOption =
    options.find((option) => category(option) === "effort") ??
    options.find((option) => category(option) === "thought_level");
  const effort =
    composerOptionLabel(effortOption) ??
    (config.confirmedEffort ? capitalizeLabel(config.confirmedEffort) : null);
  const id =
    modelOption?.type === "select"
      ? modelOption.currentValue
      : config.confirmedModel?.id;
  return { model, effort, id };
}

/**
 * Records what `chatId` inherits from its parent's session configuration.
 * A fork continues the parent's session, so it inherits model and effort. A
 * side chat starts a new session on `startsOnModelId` with no effort, so it
 * inherits only the model label, and only when that is the parent's
 * confirmed model: the label never names a model the side chat won't use.
 */
export function inheritComposerModel(
  chatId: string,
  parentConfig: AcpSessionConfig | null | undefined,
  side?: { startsOnModelId: string | null },
) {
  const confirmed = confirmedComposerModel(parentConfig);
  if (
    !confirmed ||
    (side && (!confirmed.id || side.startsOnModelId !== confirmed.id))
  )
    return;
  const inherited = side ? { ...confirmed, effort: null } : confirmed;
  useInheritedStore.setState(
    (state) =>
      Object.fromEntries([
        ...Object.entries(state)
          .filter(([id]) => id !== chatId)
          .slice(1 - MAX_INHERITED),
        [chatId, inherited],
      ]),
    true,
  );
}

export function useInheritedComposerModel(chatId: string | null) {
  return useInheritedStore((state) => (chatId ? state[chatId] : undefined));
}
