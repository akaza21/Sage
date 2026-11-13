import { invokeNative } from "./nativeInvoke";

export type NotificationPreference = "off" | "backgroundTurns";
export interface NotificationChat {
  projectId: string;
  chatId: string;
}
export interface NotificationVisibility {
  generation: number;
  revision: number;
  documentVisible: boolean;
  chatIds: readonly string[];
}
export interface NotificationPort {
  getPreference(): Promise<NotificationPreference>;
  setPreference(value: NotificationPreference): Promise<NotificationPreference>;
  beginVisibility(): Promise<number>;
  reportVisibility(update: NotificationVisibility): Promise<void>;
  takeChat(): Promise<NotificationChat | null>;
  listenOpen(wake: () => void): Promise<() => void>;
}
export const nativeNotificationPort: NotificationPort = {
  getPreference: () => invokeNative("get_notification_preference"),
  setPreference: (value) =>
    invokeNative("set_notification_preference", { value }),
  beginVisibility: () => invokeNative("begin_notification_visibility"),
  reportVisibility: (update) =>
    invokeNative("report_notification_visibility", { update }),
  takeChat: () => invokeNative("take_notification_chat"),
  async listenOpen(wake) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen("sage-notification-open-chat", wake);
  },
};
