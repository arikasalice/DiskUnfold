import { Button } from "react-aria-components";
import type { ButtonProps } from "react-aria-components";
import "../theme/row-action.css";

/**
 * A row action that appears only while its row is hovered or holds focus.
 *
 * It stays out of the tab order. Keyboard users reach it by moving focus into
 * the row's cells or through the row menu, which must offer the same action.
 */
export function RowAction({
  label,
  onPress,
}: {
  label: string;
  onPress: NonNullable<ButtonProps["onPress"]>;
}) {
  return (
    <Button
      className="du-action du-action--secondary du-row-action"
      excludeFromTabOrder
      onPress={onPress}
    >
      {label}
    </Button>
  );
}
