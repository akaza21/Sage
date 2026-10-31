import { createContext } from "react";

/**
 * Whether Explorer may rename or recycle a File that is open in a tab (ED-16).
 * The Editor window allows it, as VS Code does: the tab closes for the move
 * and reopens at the new name. Elsewhere an open File must be closed first.
 */
export const ExplorerReleasesOpenFiles = createContext(false);
