import { createContext } from "react";

/** Bound Chat slots whose resource host owns updates outside their presentation. */
export const AcpProjectionOwnerContext =
  createContext<ReadonlySet<string> | null>(null);
