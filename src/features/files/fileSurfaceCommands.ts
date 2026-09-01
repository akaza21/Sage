export interface FileSurfaceCommandActions {
  previewing: boolean;
  findOpen: boolean;
  goToOpen: boolean;
  closePreview: () => void;
  closeFind: () => void;
  closeGoToLine: () => void;
  openFind: () => void;
  openGoToLine: () => void;
  goToLocalSymbol: () => void;
  save: () => void;
}

export function handleFileSurfaceCommandKey(
  event: {
    key: string;
    altKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
    preventDefault: () => void;
  },
  actions: FileSurfaceCommandActions,
): boolean {
  if (
    event.key === "F12" &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.shiftKey
  ) {
    event.preventDefault();
    if (!actions.previewing) actions.goToLocalSymbol();
    return true;
  }
  if (event.key === "Escape") {
    if (actions.previewing) {
      event.preventDefault();
      actions.closePreview();
      return true;
    }
    if (actions.goToOpen) {
      event.preventDefault();
      actions.closeGoToLine();
      return true;
    }
    if (actions.findOpen) {
      event.preventDefault();
      actions.closeFind();
      return true;
    }
    return false;
  }
  const modified = (event.ctrlKey || event.metaKey) && !event.altKey;
  if (!modified) {
    return false;
  }
  const key = event.key.toLowerCase();
  if (key === "s") {
    event.preventDefault();
    actions.save();
    return true;
  }
  if (event.shiftKey) {
    return false;
  }
  if (key === "f") {
    event.preventDefault();
    if (!actions.previewing) {
      actions.openFind();
    }
    return true;
  }
  if (key === "g") {
    event.preventDefault();
    if (!actions.previewing) {
      actions.openGoToLine();
    }
    return true;
  }
  return false;
}
