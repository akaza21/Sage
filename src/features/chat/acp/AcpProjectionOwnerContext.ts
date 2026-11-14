import { createContext } from "react";

export const AcpProjectionOwnerContext =
  createContext<ReadonlySet<string> | null>(null);
