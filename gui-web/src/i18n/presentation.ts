import { createContext, useContext } from "react";

export type TextValues = Readonly<Record<string, string | bigint>>;
export type LocaleText = (
  key: string,
  fallback: string,
  values?: TextValues,
) => string;

export function interpolate(
  template: string,
  value: (name: string) => string,
): string {
  return template.replace(
    /\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*)\}/g,
    (match: string, name?: string) =>
      name === undefined ? match.slice(0, 1) : value(name),
  );
}

const baselineText: LocaleText = (key, fallback, values) =>
  interpolate(fallback || key, (name) => String(values?.[name] ?? "Unknown"));

export const LocaleTextContext = createContext<LocaleText>(baselineText);

export function useLocaleText(): LocaleText {
  return useContext(LocaleTextContext);
}
