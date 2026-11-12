import { readFile, readdir } from "node:fs/promises";
import ts from "typescript";

const root = new URL("../", import.meta.url);
const resources = new URL("../user-env/i18n/", root);
const languageIds = ["en", "ko"];

async function readBundle(language) {
  const data = JSON.parse(
    await readFile(new URL(`${language}.json`, resources), "utf8"),
  );
  const templates = new Map();
  for (const [key, , parameters] of data[5]) {
    if (templates.has(key))
      throw new Error(`${language} has a duplicate message key: ${key}`);
    templates.set(
      key,
      JSON.stringify(
        [...parameters].sort(([left], [right]) => left.localeCompare(right)),
      ),
    );
  }
  return templates;
}

function declaredKeys(source) {
  const keys = new Map();
  for (const line of source.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const [key, ...parameters] = line.trim().split(/\s+/);
    if (keys.has(key)) throw new Error(`Duplicate locale declaration: ${key}`);
    keys.set(
      key,
      JSON.stringify(
        parameters.sort().map((parameter) => parameter.split(":")),
      ),
    );
  }
  return keys;
}

async function usedKeys() {
  const folder = new URL("src/", root);
  const paths = await readdir(folder, { recursive: true });
  const used = new Set();
  for (const relative of paths) {
    if (!/\.tsx?$/.test(relative) || relative.includes("generated")) continue;
    const path = relative.replaceAll("\\", "/");
    const contents = await readFile(new URL(path, folder), "utf8");
    const file = ts.createSourceFile(
      path,
      contents,
      ts.ScriptTarget.Latest,
      true,
    );
    function visit(node) {
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "text"
      ) {
        const [key] = node.arguments;
        if (key && ts.isStringLiteral(key) && key.text.startsWith("ui."))
          used.add(key.text);
      }
      ts.forEachChild(node, visit);
    }
    visit(file);
  }
  return used;
}

const [english, korean] = await Promise.all(languageIds.map(readBundle));
const declared = declaredKeys(
  await readFile(new URL("message-keys.txt", resources), "utf8"),
);
const used = await usedKeys();
const errors = [];
for (const key of used) {
  if (!declared.has(key))
    errors.push(`GUI message key is not declared: ${key}`);
}
for (const [key, parameters] of declared) {
  if (!english.has(key) || !korean.has(key))
    errors.push(`Declared key is missing from a bundle: ${key}`);
  else if (english.get(key) !== parameters || korean.get(key) !== parameters)
    errors.push(`Parameter declarations differ: ${key}`);
}
for (const [index, bundle] of [english, korean].entries()) {
  for (const key of bundle.keys()) {
    if (!declared.has(key))
      process.stderr.write(`Extra key in ${languageIds[index]}: ${key}\n`);
  }
}
if (errors.length > 0) {
  process.stderr.write(`${errors.join("\n")}\n`);
  process.exitCode = 1;
} else {
  process.stdout.write(
    `Checked ${declared.size} declared locale keys and ${used.size} GUI message references.\n`,
  );
}
