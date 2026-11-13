import { type EditorSettings } from "../domain/editorSettingsCatalogue";
import {
  type NewChatDefaults,
  type NewChatDefaultsState,
} from "../domain/newChatDefaults";
import { type SessionAccessModes } from "../domain/sessionAccessMode";
import { type UiAppearance } from "../domain/uiAppearance";
import { invokeNative } from "./nativeInvoke";
import { type SettingsPort } from "./workbenchPorts";

export const nativeSettingsPort: SettingsPort = {
  getEditorSettings(): Promise<Record<string, unknown>> {
    return invokeNative("get_editor_settings");
  },
  setEditorSettings(
    changes: Partial<EditorSettings>,
  ): Promise<Record<string, unknown>> {
    return invokeNative("set_editor_settings", { changes });
  },
  getNewChatDefaults(): Promise<NewChatDefaultsState> {
    return invokeNative("get_new_chat_defaults");
  },
  setNewChatDefaults(defaults: NewChatDefaults): Promise<NewChatDefaultsState> {
    return invokeNative("set_new_chat_defaults", { defaults });
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
