import type { AnalysisClient, LocaleMissingEntry } from "../ipc";

/** Keeps diagnostics off the interaction path and sends no interpolation values. */
export function makeLocaleReporter(client: AnalysisClient) {
  const pending = new Map<string, Map<string, LocaleMissingEntry>>();
  const seen = new Set<string>();
  let frame: number | undefined;

  function send() {
    frame = undefined;
    for (const [language, entries] of pending) {
      void client
        .call("i18n.report_missing", {
          language,
          entries: [...entries.values()],
        })
        .catch(() => undefined);
    }
    pending.clear();
  }

  return {
    note(language: string, key: string, marker: LocaleMissingEntry["marker"]) {
      const id = `${language}\0${key}`;
      if (seen.has(id)) return;
      seen.add(id);
      if (seen.size > 1000) {
        const oldest = seen.values().next().value;
        if (oldest !== undefined) seen.delete(oldest);
      }
      let entries = pending.get(language);
      if (!entries) {
        entries = new Map();
        pending.set(language, entries);
      }
      entries.set(key, { key, marker });
      frame ??= requestAnimationFrame(send);
    },
    cancel() {
      if (frame !== undefined) cancelAnimationFrame(frame);
      frame = undefined;
      pending.clear();
      seen.clear();
    },
  };
}
