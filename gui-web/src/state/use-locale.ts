import { useEffect, useMemo, useSyncExternalStore } from "react";
import { AnalysisError } from "../ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  LocaleBundleBody,
  LocaleProfile,
  LocaleProfileChanges,
} from "../ipc";

type BundleEntry = { revision: string; body: LocaleBundleBody };
export type LocaleView = {
  profile: LocaleProfile;
  baseline: LocaleBundleBody;
  selected: LocaleBundleBody;
  language: string;
};

type LocaleState = {
  view?: LocaleView;
  errorKey?: string;
  commandError?: { code: string; reasonKey: string };
};

const initialState: LocaleState = {};
const baselineLanguage = "en";
const loadError = "ui.locale.failed";

function compareRevision(left: string, right: string) {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function localeRevision(event: EventEnvelope) {
  const body: unknown = event.body;
  if (
    typeof body === "object" &&
    body !== null &&
    "profileRevision" in body &&
    typeof body.profileRevision === "string" &&
    /^(0|[1-9][0-9]{0,19})$/.test(body.profileRevision)
  ) {
    return body.profileRevision;
  }
  return undefined;
}

function makeLocaleSession(client: AnalysisClient) {
  let state = initialState;
  let active = false;
  let request = 0;
  let previousSeq: number | undefined;
  let saving = false;
  const cache = new Map<string, BundleEntry>();
  const listeners = new Set<() => void>();

  function publish(next: LocaleState) {
    state = next;
    for (const listener of listeners) listener();
  }

  async function bundle(profile: LocaleProfile, language: string) {
    const expected = profile.supportedLanguages.find(
      (entry) => entry.language === language,
    )?.bundleRevision;
    if (expected === undefined) throw new Error("Missing registered language");
    const cached = cache.get(language);
    if (cached?.revision === expected) return cached.body;
    const result = await client.call("i18n.get_bundle", {
      language,
      ...(cached && { knownBundleRevision: cached.revision }),
    });
    if (result.bundleRevision !== expected || !result.bundle)
      throw new Error("Unexpected language bundle revision");
    cache.set(language, { revision: expected, body: result.bundle });
    return result.bundle;
  }

  async function loadView(profile: LocaleProfile): Promise<LocaleState> {
    const baseline = await bundle(profile, baselineLanguage);
    let language = profile.displayLanguage;
    let selected = baseline;
    let errorKey: string | undefined;
    if (language !== baselineLanguage) {
      try {
        selected = await bundle(profile, language);
      } catch {
        language = baselineLanguage;
        errorKey = loadError;
      }
    }
    return {
      view: { profile, baseline, selected, language },
      ...(errorKey && { errorKey }),
    };
  }

  async function refresh(
    errorKey?: string,
    commandError?: LocaleState["commandError"],
  ) {
    const ticket = ++request;
    try {
      const profile = await client.call("i18n.get_profile", {});
      const current = state.view?.profile;
      if (
        current &&
        compareRevision(profile.profileRevision, current.profileRevision) < 0
      )
        return;
      const loaded = await loadView(profile);
      if (active && ticket === request)
        publish({
          ...loaded,
          ...(errorKey && { errorKey }),
          ...(commandError && { commandError }),
        });
    } catch {
      if (active && ticket === request)
        publish({
          ...state,
          errorKey: errorKey ?? loadError,
          ...(commandError && { commandError }),
        });
    }
  }

  function receive(event: EventEnvelope) {
    const gap = previousSeq !== undefined && event.seq !== previousSeq + 1;
    previousSeq = event.seq;
    if (gap || event.topic === "job.resync_required") {
      void refresh();
      return;
    }
    if (event.topic !== "locale.changed") return;
    const revision = localeRevision(event);
    const current = state.view?.profile.profileRevision;
    if (
      revision === undefined ||
      current === undefined ||
      compareRevision(revision, current) > 0
    )
      void refresh();
  }

  async function save(changes: LocaleProfileChanges) {
    const expectedProfileRevision = state.view?.profile.profileRevision;
    if (saving || expectedProfileRevision === undefined) return;
    saving = true;
    try {
      await client.call("i18n.set_profile", {
        expectedProfileRevision,
        changes,
      });
      if (active) await refresh();
    } catch (error) {
      if (active) {
        if (
          error instanceof AnalysisError &&
          error.reasonKey === "locale.profile_modified"
        ) {
          await refresh("ui.locale.modified", error);
        } else {
          publish({
            ...state,
            errorKey: "ui.locale.save_failed",
            ...(error instanceof AnalysisError && { commandError: error }),
          });
        }
      }
    } finally {
      saving = false;
    }
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    snapshot: () => state,
    start() {
      active = true;
      previousSeq = undefined;
      const unsubscribe = client.subscribe(receive);
      void refresh();
      return () => {
        active = false;
        request += 1;
        unsubscribe();
      };
    },
    retry: () => {
      void refresh();
    },
    save: (changes: LocaleProfileChanges) => {
      void save(changes);
    },
  };
}

export function useLocale(client: AnalysisClient) {
  const session = useMemo(() => makeLocaleSession(client), [client]);
  const state = useSyncExternalStore(session.subscribe, session.snapshot);
  useEffect(() => session.start(), [session]);
  return { ...state, retry: session.retry, save: session.save };
}
