export function variantDeclarationErrors(declared) {
  const errors = [];
  for (const [key, parameters] of declared) {
    if (!key.startsWith("ui.")) continue;
    const match = key.match(/^(.*)\.(?:unix|macos|linux)$/);
    if (!match?.[1]) continue;
    const base = match[1];
    if (!declared.has(base))
      errors.push(`Locale variant is missing base key: ${key}`);
    else if (declared.get(base) !== parameters)
      errors.push(`Locale variant parameters differ from base key: ${key}`);
  }
  return errors;
}
