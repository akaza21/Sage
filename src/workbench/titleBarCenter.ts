import { chatDisplayTitle } from "../domain/chatTitle";

const TITLE_BAR_LABEL_CAP = 48;

export function titleBarSessionTitle(
  tabTitle: string | null | undefined,
): string | null {
  if (!tabTitle) return null;
  const name = chatDisplayTitle(tabTitle).trim();
  if (!name || /\p{Cc}/u.test(name)) return null;
  if (name.length <= TITLE_BAR_LABEL_CAP) return name;
  return `${name.slice(0, TITLE_BAR_LABEL_CAP - 1)}…`;
}
