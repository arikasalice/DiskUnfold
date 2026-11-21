import { createContext } from "react";
import type { AnalysisClient, ScanCollection } from "../../ipc";

export type ScanResourceOwner = {
  client: AnalysisClient;
  jobRef: string | undefined;
  collection: ScanCollection | undefined;
  active: boolean;
};

export const ScanResourceContext = createContext<ScanResourceOwner | undefined>(
  undefined,
);
