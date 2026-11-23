import type { FocusEventHandler, PointerEventHandler } from "react";
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
  title,
  tabIndex,
  onPointerEnter,
  onFocus,
}: {
  name: string;
  item: boolean;
  folder?: boolean;
  title?: string;
  tabIndex?: number;
  onPointerEnter?: PointerEventHandler<HTMLElement>;
  onFocus?: FocusEventHandler<HTMLElement>;
}) {
  if (!item) return name;
  const interactionProps = {
    ...(title === undefined ? {} : { title }),
    ...(tabIndex === undefined ? {} : { tabIndex }),
    ...(onPointerEnter ? { onPointerEnter } : {}),
    ...(onFocus ? { onFocus } : {}),
  };
  // The bar label's 14 px icon, 4 px before the name and centred on its line.
  return folder ? (
    <div className="du-row-name" {...interactionProps}>
      <Icon name="folder" size={14} />
      <NameText name={name} />
    </div>
  ) : Object.keys(interactionProps).length > 0 ? (
    <div {...interactionProps}>
      <NameText name={name} />
    </div>
  ) : (
    <NameText name={name} />
  );
}
