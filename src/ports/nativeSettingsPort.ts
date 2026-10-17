import type { SettingsPort } from "./workbenchPorts";
import type { EditorAppearance } from "../features/files/editorAppearance";
import type { SessionAccessModes } from "../domain/sessionAccessMode";
import type { UiAppearance } from "../domain/uiAppearance";
import { invokeNative } from "./nativeInvoke";

export const nativeSettingsPort: SettingsPort = {
  getEditorAppearance(): Promise<EditorAppearance> {
    return invokeNative("get_editor_appearance");
  },
  setEditorAppearance(appearance: EditorAppearance): Promise<EditorAppearance> {
    return invokeNative("set_editor_appearance", { appearance });
  },
  getSessionAccessModes(): Promise<SessionAccessModes> {
    return invokeNative("get_session_access_modes");
  },
  setSessionAccessModes(
    modes: SessionAccessModes,
  ): Promise<SessionAccessModes> {
    return invokeNative("set_session_access_modes", { modes });
  },
  getUiAppearance(): Promise<UiAppearance> {
    return invokeNative("get_ui_appearance");
  },
  setUiAppearance(appearance: UiAppearance): Promise<UiAppearance> {
    return invokeNative("set_ui_appearance", { appearance });
  },
};
