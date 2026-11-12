import type { ReactNode } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { Action } from "./controls";
import { useLocaleText } from "../i18n/presentation";
import "../theme/inspection-dialog.css";

/** Presents read-only details. In short viewports, the title and body scroll together. */
export function InspectionDialog({
  title,
  onClose,
  children,
  closeLabel,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  closeLabel?: string;
}) {
  const text = useLocaleText();
  return (
    <ModalOverlay
      className="du-inspection-overlay"
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Modal className="du-inspection-modal">
        <Dialog className="du-inspection-dialog">
          <header className="du-inspection-dialog__header">
            <Heading slot="title" className="du-heading du-heading--2">
              {title}
            </Heading>
            <Action variant="quiet" autoFocus onPress={onClose}>
              {closeLabel ?? text("ui.common.close_details", "Close details")}
            </Action>
          </header>
          <section
            className="du-inspection-dialog__body"
            aria-label={text("ui.common.details", "Details")}
          >
            {children}
          </section>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
