import { Icon, NameText } from "../components";

/**
 * The name shown for a visualization row. Only an item row carries the name
 * of a file or folder, so only it marks edge white space; the label the
 * engine gives a group row is drawn as given. `name` stays the row's real
 * display text. A folder starts with the folder icon, as its bar does (#318).
 */
export function RowName({
  name,
  item,
  folder = false,
}: {
  name: string;
  item: boolean;
  folder?: boolean;
}) {
  if (!item) return name;
  return folder ? (
    <>
      <Icon name="folder" />
      <NameText name={name} />
    </>
  ) : (
    <NameText name={name} />
  );
}
