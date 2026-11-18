import { Button } from "react-aria-components";
import type { ButtonProps } from "react-aria-components";
import type { ReactNode } from "react";
import "../theme/row-action.css";

/**
 * A row action that appears only while its row is hovered or holds focus.
 *
 * It stays out of the tab order. Keyboard users reach it by moving focus into
 * the row's cells or through the row menu, which must offer the same action.
 *
 * Pressing it moves focus to its row, where keyboard navigation of the list
 * keeps focus, because the action usually goes away once it has run and the
 * browser would otherwise leave focus on the document body.
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
      onPress={(event) => {
        const row = event.target.closest<HTMLElement>('[role="row"]');
        onPress(event);
        row?.focus({ preventScroll: true });
      }}
    >
      {label}
    </Button>
  );
}

/**
 * A place at the end of a row's name that holds one row status badge or one
 * {@link RowAction}, or stays empty.
 *
 * Every row reserves the width of the widest of `badges` (drawn as info
 * badges) and `actions` (drawn as row actions), so neither the name nor the
 * controls after the place move when an action appears on the hovered or
 * focused row. The reserved texts are drawn from attributes of a hidden
 * element, so they are neither document text nor part of the accessibility
 * tree; only `children` are read. `holds` names what `children` is, which
 * lets a narrow layout drop an empty place or an action it cannot show.
 */
export function RowSlot({
  badges,
  actions,
  holds,
  children,
}: {
  badges: readonly string[];
  actions: readonly string[];
  holds: "badge" | "action" | undefined;
  children?: ReactNode;
}) {
  return (
    <span className="du-row-slot" data-holds={holds}>
      <span className="du-row-slot__reserve" aria-hidden="true">
        {badges.map((label) => (
          <span
            key={`badge:${label}`}
            className="du-badge du-row-slot__badge"
            data-label={label}
          />
        ))}
        {actions.map((label) => (
          <span
            key={`action:${label}`}
            className="du-action du-action--secondary du-row-slot__action"
            data-label={label}
          />
        ))}
      </span>
      {children}
    </span>
  );
}
