import { waitFor } from "@testing-library/react";

/** Tests find warnings by severity; only announcing ones are alerts. */
export function queryWarning(root: ParentNode = document): HTMLElement | null {
  return root.querySelector<HTMLElement>(".notice--warning");
}

export function getWarning(root: ParentNode = document): HTMLElement {
  const notice = queryWarning(root);
  if (!notice) throw new Error("No warning notice is shown");
  return notice;
}

export async function findWarning(root: ParentNode = document) {
  return waitFor(() => getWarning(root));
}
