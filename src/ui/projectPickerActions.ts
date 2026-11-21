import { Plus, X } from "./leanIcons";

/** The same project-creation command is offered by the picker and search palette. */
export const NEW_PROJECT_ACTION = {
  id: "scope:new",
  label: "New project",
  leadingIcon: Plus,
  separatorBefore: true,
  alwaysVisible: true,
};

export const NO_PROJECT_ACTION = {
  id: "scope:none",
  label: "Don't work in a project",
  leadingIcon: X,
  alwaysVisible: true,
};
