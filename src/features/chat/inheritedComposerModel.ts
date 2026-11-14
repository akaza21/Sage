import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { sessionConfigControls } from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { composerOptionLabel } from "./composerChrome";
import {
  useInheritedStore,
  type InheritedComposerModel,
} from "./inheritedComposerModelState";

export {
  useInheritedComposerModel,
  type InheritedComposerModel,
} from "./inheritedComposerModelState";

/** Side chats are capped at eight; forks are short-lived until first send. */
const MAX_INHERITED = 16;

const category = (option: AcpConfigOption) =>
  (option.category ?? option.id).toLocaleLowerCase();

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
