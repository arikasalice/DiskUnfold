import type { BreadcrumbEntry } from "../../components";
import type { LocaleText } from "../../i18n/presentation";
import type { BrowseOpenResponse, BrowseValue, ItemRef } from "../../ipc";

type Opened = Extract<BrowseOpenResponse, { kind: "opened" }>;

export function browseDisplayName(
  value: BrowseValue<string>,
  text: LocaleText = (key, fallback) => fallback || key,
): string {
  return value.state === "present"
    ? value.value
    : value.state === "absent"
      ? text("ui.browse.not_recorded", "Not recorded")
      : text("ui.browse.unavailable", "Unavailable");
}

export function rowKey(item: ItemRef): string {
  return `${item.resultVersion}:${item.itemNo}`;
}

export function folderPath(
  opened: Opened,
  knownVirtualRoot: boolean,
  open: (location?: ItemRef) => void,
  text: LocaleText = (key, fallback) => fallback || key,
  rootLabel?: string,
): BreadcrumbEntry[] {
  const root = {
    id: "scope-roots",
    label: rootLabel ?? text("ui.browse.scope_roots", "Scope roots"),
  };
  if (opened.rootKind === "virtual_root") return [{ ...root, current: true }];
  const entries: BreadcrumbEntry[] = [];
  if (knownVirtualRoot) entries.push({ ...root, onPress: () => open() });

  const { location, breadcrumbs, page } = opened;
  const current = breadcrumbs.at(-1);
  const unavailable = {
    id: "path-unavailable",
    label: text("ui.browse.folder_path_unavailable", "Folder path unavailable"),
  };
  if (
    !current ||
    location?.resultVersion !== page.resultVersion ||
    rowKey(current.item) !== rowKey(location)
  )
    return [...entries, unavailable];

  const ancestors = breadcrumbs.slice(0, -1);
  // A supplied root label names the single scope root (for example, sample data)
  // instead of its native path; multi-root results already show it at the virtual root.
  const crumbLabel = (index: number, value: BrowseValue<string>) =>
    rootLabel !== undefined && !knownVirtualRoot && index === 0
      ? rootLabel
      : browseDisplayName(value, text);
  if (
    ancestors.some(
      ({ item }) =>
        item.resultVersion !== page.resultVersion ||
        rowKey(item) === rowKey(location),
    )
  ) {
    entries.push(unavailable);
  } else {
    for (const [index, crumb] of ancestors.entries()) {
      entries.push({
        id: rowKey(crumb.item),
        label: crumbLabel(index, crumb.displayName),
        onPress: () => open(crumb.item),
      });
    }
  }
  entries.push({
    id: rowKey(current.item),
    label: crumbLabel(breadcrumbs.length - 1, current.displayName),
    current: true,
  });
  return entries;
}
