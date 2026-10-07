import type { SettingsPort } from "./workbenchPorts";
import type { EditorAppearance } from "../features/files/editorAppearance";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import type { UiAppearance } from "../domain/uiAppearance";
import { invokeNative } from "./nativeInvoke";

export const nativeSettingsPort: SettingsPort = {
  getEditorAppearance(): Promise<EditorAppearance> {
    return invokeNative("get_editor_appearance");
  },
  setEditorAppearance(appearance: EditorAppearance): Promise<EditorAppearance> {
    return invokeNative("set_editor_appearance", { appearance });
  },
  getSessionAccessMode(): Promise<SessionAccessMode> {
    return invokeNative("get_session_access_mode");
  },
  setSessionAccessMode(mode: SessionAccessMode): Promise<SessionAccessMode> {
    return invokeNative("set_session_access_mode", { mode });
  },
  getUiAppearance(): Promise<UiAppearance> {
    return invokeNative("get_ui_appearance");
  },
  setUiAppearance(appearance: UiAppearance): Promise<UiAppearance> {
    return invokeNative("set_ui_appearance", { appearance });
  },
};
