import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Button,
  Menu,
  MenuItem,
  MenuTrigger,
  Popover,
} from "react-aria-components";
import "../theme/item-actions.css";

export type ItemAction = {
  id: string;
  label: string;
  onAction: (trigger: HTMLElement | null) => void;
  disabledReason?: string;
};

/** The same actions are available by right click, Shift+F10 and a visible menu. */
export function ItemActions({
  label,
  actions,
  children,
  rowScope = false,
}: {
  label: string;
  actions: readonly ItemAction[];
  children: ReactNode;
  rowScope?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  function openMenu() {
    trigger.current?.focus({ preventScroll: true });
    setOpen(true);
  }
  useEffect(() => {
    if (!rowScope) return;
    const row = trigger.current?.closest('[role="row"]');
    if (!row) return;
    const context = (event: Event) => {
      if (
        event.target instanceof Element &&
        event.target.closest("input,textarea,[contenteditable='true']")
      )
        return;
      event.preventDefault();
      openMenu();
    };
    const key = (event: Event) => {
      if (
        !(event instanceof KeyboardEvent) ||
        event.isComposing ||
        event.repeat ||
        !(
          (event.shiftKey && event.key === "F10") ||
          event.key === "ContextMenu"
        )
      )
        return;
      event.preventDefault();
      openMenu();
    };
    row.addEventListener("contextmenu", context);
    row.addEventListener("keydown", key);
    return () => {
      row.removeEventListener("contextmenu", context);
      row.removeEventListener("keydown", key);
    };
  }, [rowScope]);
  return (
    <div
      className={`du-item-actions${rowScope ? " du-item-actions--row" : ""}`}
      onContextMenu={(event) => {
        if (rowScope) return;
        if (
          event.target instanceof Element &&
          event.target.closest("input,textarea,[contenteditable='true']")
        )
          return;
        event.preventDefault();
        openMenu();
      }}
      onKeyDown={(event) => {
        if (rowScope) return;
        if (
          !event.nativeEvent.isComposing &&
          !event.repeat &&
          ((event.shiftKey && event.key === "F10") ||
            event.key === "ContextMenu")
        ) {
          event.preventDefault();
          openMenu();
        }
      }}
    >
      {children}
      <MenuTrigger isOpen={open} onOpenChange={setOpen}>
        <Button
          ref={trigger}
          className="du-action du-action--quiet"
          aria-label={label}
          excludeFromTabOrder={rowScope}
        >
          ⋯
        </Button>
        <Popover className="du-item-menu" placement="bottom end">
          <Menu
            aria-label={label}
            disabledKeys={actions
              .filter((x) => x.disabledReason !== undefined)
              .map((x) => x.id)}
            onAction={(id) =>
              actions
                .find((x) => x.id === id && x.disabledReason === undefined)
                ?.onAction(trigger.current)
            }
          >
            {actions.map((action) => (
              <MenuItem key={action.id} id={action.id} textValue={action.label}>
                <span>{action.label}</span>
                {action.disabledReason !== undefined && (
                  <span className="du-item-menu__reason">
                    {action.disabledReason}
                  </span>
                )}
              </MenuItem>
            ))}
          </Menu>
        </Popover>
      </MenuTrigger>
    </div>
  );
}
