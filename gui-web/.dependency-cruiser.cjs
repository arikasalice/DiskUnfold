const zone = (name) => `^src/${name}(?:/|$)`;

module.exports = {
  forbidden: [
    {
      name: "no-preview-in-product",
      severity: "error",
      from: { path: "^src/" },
      to: { path: "^preview/" },
    },
    {
      name: "no-circular",
      severity: "error",
      from: {},
      to: { circular: true },
    },
    {
      name: "no-unresolved",
      severity: "error",
      from: {},
      to: { couldNotResolve: true },
    },
    {
      name: "feature-isolation",
      severity: "error",
      from: { path: "^src/features/([^/]+)/" },
      to: { path: "^src/features/", pathNot: "^src/features/$1/" },
    },
    {
      name: "components-pure",
      severity: "error",
      from: { path: zone("components") },
      to: { path: zone("(features|app|state|ipc)") },
    },
    {
      name: "native-bridge-only-in-ipc",
      severity: "error",
      from: { pathNot: zone("ipc") },
      to: { path: "^src/ipc/bridge\\.ts$" },
    },
    {
      name: "generated-via-ipc-index",
      severity: "error",
      from: { pathNot: "^src/ipc/(index\\.ts$|bridge\\.ts$|generated/)" },
      to: { path: zone("ipc/generated") },
    },
    {
      name: "state-no-ui",
      severity: "error",
      from: { path: zone("state") },
      to: { path: zone("(app|features|components)") },
    },
    {
      name: "app-is-root",
      severity: "error",
      from: { pathNot: "^src/(main\\.tsx$|app/)" },
      to: { path: zone("app") },
    },
    {
      name: "leaf-i18n-theme",
      severity: "error",
      from: { path: zone("(i18n|theme)") },
      to: { path: "^src/", pathNot: zone("(i18n|theme)") },
    },
  ],
  options: {
    doNotFollow: { path: "node_modules" },
    tsPreCompilationDeps: true,
    tsConfig: { fileName: "tsconfig.json" },
  },
};
