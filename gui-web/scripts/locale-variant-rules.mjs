const VARIANT = /^(.*)\.(?:unix|macos|linux)$/;

export function variantDeclarationErrors(declared) {
  const errors = [];
  for (const [key, parameters] of declared) {
    if (!key.startsWith("ui.")) continue;
    const match = key.match(VARIANT);
    if (!match?.[1]) continue;
    const base = match[1];
    if (!declared.has(base))
      errors.push(`Locale variant is missing base key: ${key}`);
    else if (declared.get(base) !== parameters)
      errors.push(`Locale variant parameters differ from base key: ${key}`);
  }
  return errors;
}

/**
 * Every declared key must be in every bundle, so a variant key that a bundle
 * has but the declarations lack is the only way the languages' variant sets
 * can differ. `bundles` is a list of [language, keys] pairs.
 */
export function variantBundleErrors(declared, bundles) {
  const errors = [];
  for (const [language, keys] of bundles) {
    for (const key of keys) {
      if (key.startsWith("ui.") && VARIANT.test(key) && !declared.has(key))
        errors.push(`Locale variant is only in the ${language} bundle: ${key}`);
    }
  }
  return errors;
}
