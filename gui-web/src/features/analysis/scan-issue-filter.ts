export type ScanIssueLaneSelection = {
  rootIndexes?: number[];
  storagePath?: string;
  storageTail?: string;
  storageLabel: string;
  storagePaths: string[];
  allStorages: boolean;
};

export type ScanIssueLaneRequest = ScanIssueLaneSelection & {
  requestId: number;
};
