import { createContext, useContext } from "react";

/**
 * Counts requests to open the support information dialog.
 *
 * The shell increments the count when a feature screen, such as the startup
 * banner, asks for the support screen. `InfoEntry` opens its dialog on each
 * increase, so features never import the screen they point to.
 */
export const SupportRequestContext = createContext(0);

/** The number of support information requests made so far. */
export function useSupportRequests(): number {
  return useContext(SupportRequestContext);
}
