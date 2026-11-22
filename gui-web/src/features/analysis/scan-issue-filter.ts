import type { ItemRef } from "../../ipc";

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
  /** Restricts the list to this folder and everything below it (F02 #358). */
  folder?: ScanIssueFolder;
};

/** The folder a scan issue list is restricted to, with the name its chip shows. */
export type ScanIssueFolder = {
  item: ItemRef;
  label: string;
  /** The folder's full display path, when it could be read; the chip's tooltip. */
  path?: string | undefined;
};

/** A request from Browse to show the unreadable locations below one folder. */
export type ScanIssueFolderRequest = ScanIssueFolder & { requestId: number };
