import { NameText } from "../components";

/**
 * The name shown for a visualization row. Only an item row carries the name
 * of a file or folder, so only it marks edge white space; the label the
 * engine gives a group row is drawn as given. `name` stays the row's real
 * display text.
 */
export function RowName({ name, item }: { name: string; item: boolean }) {
  return item ? <NameText name={name} /> : name;
}
