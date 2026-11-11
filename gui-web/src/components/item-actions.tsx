import { useRef, useState } from "react";
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
  onAction: () => void;
  disabledReason?: string;
};

/** The same actions are available by right click, Shift+F10 and a visible menu. */
export function ItemActions({
  label,
  actions,
  children,
}: {
  label: string;
  actions: readonly ItemAction[];
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  function openMenu() {
    trigger.current?.focus({ preventScroll: true });
    setOpen(true);
  }
  return (
    <div
      className="du-item-actions"
      onContextMenu={(event) => {
        if (
          event.target instanceof Element &&
          event.target.closest("input,textarea,[contenteditable='true']")
        )
          return;
        event.preventDefault();
        openMenu();
      }}
      onKeyDown={(event) => {
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
                ?.onAction()
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
