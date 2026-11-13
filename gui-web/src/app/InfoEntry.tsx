import { useEffect, useRef, useState } from "react";
import { Action, InspectionDialog } from "../components";
import { ProductScreen } from "../features/product/ProductScreen";
import { SupportScreen } from "../features/support/SupportScreen";
import { useLocaleText } from "../i18n/presentation";
import type { AnalysisClient } from "../ipc";
import { useSupportRequests } from "./support-request";
import "../theme/info-screens.css";

type Open = "product" | "support";

/**
 * One-click entry to the product and support screens (F15 FR5.HD2, FR6.HD2).
 *
 * Both screens open in a dialog that returns focus to the button that opened
 * it. The shell shows this entry in the header, and on the startup screen
 * where there is no header.
 * A request from `SupportRequestContext` opens the support dialog too.
 */
export function InfoEntry({
  client,
  fallbackClient,
  standalone = false,
}: {
  client: AnalysisClient | undefined;
  /** Used when analysis is off but the host connection still serves product information. */
  fallbackClient?: AnalysisClient | undefined;
  standalone?: boolean;
}) {
  const text = useLocaleText();
  const [open, setOpen] = useState<Open>();
  const requests = useSupportRequests();
  const handled = useRef(requests);
  useEffect(() => {
    if (requests === handled.current) return;
    handled.current = requests;
    setOpen("support");
  }, [requests]);
  const product = text("ui.info.product_open", "About DiskUnfold");
  const support = text("ui.info.support_open", "Support information");
  const host = client ?? fallbackClient;
  if (!host) return null;
  return (
    <nav
      className={
        standalone ? "du-info-entry du-info-entry--standalone" : "du-info-entry"
      }
      aria-label={text("ui.info.nav", "Product and support")}
    >
      <Action variant="quiet" onPress={() => setOpen("product")}>
        {product}
      </Action>
      <Action variant="quiet" onPress={() => setOpen("support")}>
        {support}
      </Action>
      {open && (
        <InspectionDialog
          title={open === "product" ? product : support}
          onClose={() => setOpen(undefined)}
        >
          {open === "product" ? (
            <ProductScreen client={host} />
          ) : (
            <SupportScreen client={host} />
          )}
        </InspectionDialog>
      )}
    </nav>
  );
}
