import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { inventory } from "./npm-licenses.mjs";

function fixture() {
  const root = resolve("gui-web");
  const pkg = { version: "1.0.0", license: "MIT", integrity: "sha512-fixture", resolved: "https://registry.npmjs.org/example.tgz" };
  const lock = { packages: {
    "": {}, "node_modules/react": pkg,
    "node_modules/platform": { ...pkg, optional: true },
    "node_modules/compiler-alias": { ...pkg, name: "compiler", dev: true },
  } };
  const sbom = { bomFormat: "CycloneDX", components: ["react", "platform", "compiler-alias"].map(name => ({
    name, version: pkg.version, licenses: [{ license: { id: "MIT" } }],
  })) };
  const installed = { dependencies: { react: { name: "react", version: pkg.version, path: resolve(root, "node_modules/react") } } };
  return { root, lock, sbom, installed };
}

test("checks absent platform dependencies and preserves aliases", () => {
  const { root, lock, sbom, installed } = fixture();
  const packages = inventory(lock, sbom, installed, root);
  assert.deepEqual(packages.map(p => [p.alias, p.name, p.production]), [
    ["react", "react", true], ["platform", "platform", true], ["compiler-alias", "compiler", false],
  ]);
});

test("rejects omitted components, conflicting licenses and the wrong installed path", () => {
  const { root, lock, sbom, installed } = fixture();
  const react = sbom.components.shift();
  assert.throws(() => inventory(lock, sbom, installed, root), /locked license for react/);
  sbom.components.push(react, { ...react, licenses: [{ license: { id: "GPL-3.0-only" } }] });
  assert.throws(() => inventory(lock, sbom, installed, root), /Conflicting CycloneDX/);
  sbom.components.pop();
  installed.dependencies.react.path = resolve(root, "node_modules/platform");
  assert.throws(() => inventory(lock, sbom, installed, root), /Unexpected installed production package/);
});
