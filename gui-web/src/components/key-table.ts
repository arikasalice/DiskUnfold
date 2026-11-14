/** A modifier key in a binding's modifier set. */
export type KeyModifier = "alt" | "ctrl" | "meta" | "shift";

/**
 * One row of a screen's key assignment table.
 *
 * `key` is the logical key value (`KeyboardEvent.key`), so a letter binding
 * follows the user's layout. A physical position (`KeyboardEvent.code`) is
 * never a binding. `descriptionKey` names the message that explains the key.
 */
export type KeyBinding = Readonly<{
  screen: string;
  key: string;
  modifiers: readonly KeyModifier[];
  action: string;
  descriptionKey: string;
}>;

/** The subset of a keyboard event that binding lookup reads. */
export type KeyEventLike = Readonly<{
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}>;

/** A built key assignment table. Build one with {@link createKeyTable}. */
export type KeyTable = Readonly<{
  bindings: readonly KeyBinding[];
  /** The binding that exactly matches the event on the screen, if any. */
  find: (screen: string, event: KeyEventLike) => KeyBinding | undefined;
  /** Whether any binding on the screen uses this logical key. */
  usesKey: (screen: string, key: string) => boolean;
}>;

/** Thrown when two rows of one screen assign the same key and modifiers. */
export class DuplicateKeyBindingError extends Error {
  constructor(readonly duplicates: readonly (readonly KeyBinding[])[]) {
    super(
      `Duplicate key bindings: ${duplicates
        .map((group) => bindingSignature(group[0]!))
        .join(", ")}`,
    );
    this.name = "DuplicateKeyBindingError";
  }
}

/** Letters compare case-insensitively; every other key compares as written. */
export function normalizeLogicalKey(key: string): string {
  return key.length === 1 ? key.toLowerCase() : key;
}

function modifierSet(binding: readonly KeyModifier[]): string {
  return [...new Set(binding)].sort().join("+");
}

/** A stable identity: the screen, the logical key and the modifier set. */
export function bindingSignature(binding: KeyBinding): string {
  return `${binding.screen}|${modifierSet(binding.modifiers)}|${normalizeLogicalKey(binding.key)}`;
}

/** Groups of rows that share one signature. Empty when the rows are valid. */
export function findDuplicateBindings(
  rows: readonly KeyBinding[],
): KeyBinding[][] {
  const groups = new Map<string, KeyBinding[]>();
  for (const row of rows) {
    const signature = bindingSignature(row);
    groups.set(signature, [...(groups.get(signature) ?? []), row]);
  }
  return [...groups.values()].filter((group) => group.length > 1);
}

function eventModifiers(event: KeyEventLike, ignoreShift: boolean): string {
  const held: KeyModifier[] = [];
  if (event.altKey) held.push("alt");
  if (event.ctrlKey) held.push("ctrl");
  if (event.metaKey) held.push("meta");
  if (event.shiftKey && !ignoreShift) held.push("shift");
  return modifierSet(held);
}

/** Shift is part of the logical value of a printable non-letter such as "?". */
function shiftIsImplicit(key: string): boolean {
  return key.length === 1 && key.toLowerCase() === key.toUpperCase();
}

const registered: KeyTable[] = [];

/** Every table built so far, for the static duplicate check. */
export function registeredKeyTables(): readonly KeyTable[] {
  return registered;
}

/**
 * Builds and registers a key assignment table.
 *
 * Throws {@link DuplicateKeyBindingError} for a duplicate assignment on one
 * screen and `RangeError` for a row without an action or description key, so
 * a bad table fails when its module loads.
 */
export function createKeyTable(rows: readonly KeyBinding[]): KeyTable {
  for (const row of rows) {
    if (!row.action || !row.descriptionKey || !row.key || !row.screen)
      throw new RangeError(`Incomplete key binding: ${JSON.stringify(row)}`);
  }
  const duplicates = findDuplicateBindings(rows);
  if (duplicates.length > 0) throw new DuplicateKeyBindingError(duplicates);
  const table: KeyTable = {
    bindings: rows,
    find(screen, event) {
      const key = normalizeLogicalKey(event.key);
      const held = eventModifiers(event, shiftIsImplicit(event.key));
      return rows.find(
        (row) =>
          row.screen === screen &&
          normalizeLogicalKey(row.key) === key &&
          modifierSet(row.modifiers) === held,
      );
    },
    usesKey(screen, key) {
      const wanted = normalizeLogicalKey(key);
      return rows.some(
        (row) =>
          row.screen === screen && normalizeLogicalKey(row.key) === wanted,
      );
    },
  };
  registered.push(table);
  return table;
}
