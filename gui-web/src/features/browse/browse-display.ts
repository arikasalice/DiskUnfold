import type { BreadcrumbEntry } from "../../components";
import type { BrowseOpenResponse, BrowseValue, ItemRef } from "../../ipc";

type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;

export function browseDisplayName(value: BrowseValue<string>): string {
  return value.state === "present"
    ? value.value
    : value.state === "absent"
      ? "Not recorded"
      : "Unavailable";
}

export function rowKey(item: ItemRef): string {
  return `${item.resultVersion}:${item.itemNo}`;
}

export function folderPath(
  opened: Opened,
  knownVirtualRoot: boolean,
  open: (location?: ItemRef) => void,
): BreadcrumbEntry[] {
  const root = { id: "scope-roots", label: "Scope roots" };
  if (opened.rootKind === "virtual_root") return [{ ...root, current: true }];
  const entries: BreadcrumbEntry[] = [];
  if (knownVirtualRoot) entries.push({ ...root, onPress: () => open() });

  const { location, breadcrumbs, page } = opened;
  const current = breadcrumbs.at(-1);
  const unavailable = {
    id: "path-unavailable",
    label: "Folder path unavailable",
  };
  if (
    !current ||
    location?.resultVersion !== page.resultVersion ||
    rowKey(current.item) !== rowKey(location)
  )
    return [...entries, unavailable];

  const ancestors = breadcrumbs.slice(0, -1);
  if (
    ancestors.some(
      ({ item }) =>
        item.resultVersion !== page.resultVersion ||
        rowKey(item) === rowKey(location),
    )
  ) {
    entries.push(unavailable);
  } else {
    for (const crumb of ancestors) {
      entries.push({
        id: rowKey(crumb.item),
        label: browseDisplayName(crumb.displayName),
        onPress: () => open(crumb.item),
      });
    }
  }
  entries.push({
    id: rowKey(current.item),
    label: browseDisplayName(current.displayName),
    current: true,
  });
  return entries;
}
