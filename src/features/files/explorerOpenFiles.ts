import { createContext } from "react";

/** True in the Editor window: an open File's tab closes for a rename and
 * reopens at the new name. Elsewhere an open File must be closed first. */
export const ExplorerReleasesOpenFiles = createContext(false);
