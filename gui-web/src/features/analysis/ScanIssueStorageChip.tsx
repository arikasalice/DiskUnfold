import type { RefObject } from "react";
import { Action, MiddleEllipsisText, Text } from "../../components";
import { useLocaleText } from "../../i18n/presentation";

/** The storage an issue list is restricted to. */
export type StorageFilter = {
  rootIndexes: number[];
  storagePath?: string | undefined;
  storageTail?: string | undefined;
  storageLabel: string;
  storagePaths: string[];
};

/** The chip that tells the issue list is restricted to one storage and clears it. */
export function ScanIssueStorageChip({
  filter,
  chipRef,
  onClear,
}: {
  filter: StorageFilter;
  chipRef: RefObject<HTMLButtonElement | null>;
  onClear: () => void;
}) {
  const text = useLocaleText();
  const tooltip = [
    ...filter.storagePaths.slice(0, 5),
    ...(filter.storagePaths.length > 5
      ? [
          text("ui.analysis.issues_filter_paths_more", "and {count} more", {
            count: BigInt(filter.storagePaths.length - 5),
          }),
        ]
      : []),
  ].join("\n");
  return (
    <div className="du-scan-issues__chip">
      <Action
        ref={chipRef}
        aria-label={text(
          "ui.analysis.issues_filter_storage_name",
          "Storage: {value}, clear storage filter",
          { value: filter.storageLabel },
        )}
        title={tooltip}
        onPress={onClear}
      >
        <Text inline>
          {text("ui.analysis.issues_filter_storage", "Storage:")}
        </Text>
        {filter.storagePath ? (
          <Text inline className="du-scan-issues__storage-path">
            <MiddleEllipsisText text={filter.storagePath} cell />
          </Text>
        ) : (
          filter.storageLabel
        )}
        {filter.storageTail && (
          <Text inline className="du-scan-issues__storage-tail">
            {filter.storageTail}
          </Text>
        )}
        <Text inline>×</Text>
      </Action>
    </div>
  );
}
