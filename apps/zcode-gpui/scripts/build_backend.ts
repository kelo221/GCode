// Compiles the unchanged CLI app-server into one Bun executable for GPUI, so a
// packaged GPUI needs neither a repository checkout nor the Desktop runtime.
//
//   bun apps/zcode-gpui/scripts/build_backend.ts [--outdir <dir>]
//
// Output: <outdir>/zcode-backend[.exe] plus provider/zcode-builtin.json, which the CLI
// looks for beside its entry, and node_modules/ripgrep, which the file-search worker
// imports by file URL (a compiled worker cannot resolve bare package names), and
// node_modules/quickjs-wasi/quickjs.wasm for the codemode tool. The production release is staged, never the test one,
// because the CLI materializes it into the environment root that Desktop also uses.
// Mirrors packages/cli/scripts/build.mjs: TUI, playwright-core and koffi stay
// external (TUI and browser automation are unavailable in this binary; app-server
// does not load them).
import { copyFile, cp, mkdir, readFile, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { resolveBuildExternal } from "../../zcode-cli/packages/cli/scripts/build.mjs";

const root = resolve(import.meta.dirname, "../../..");
const outdirFlag = process.argv.indexOf("--outdir");
const outdir = resolve(
  outdirFlag > 0 ? process.argv[outdirFlag + 1] : join(root, "apps/zcode-gpui/target/backend"),
);
const { version } = JSON.parse(await readFile(join(root, "package.json"), "utf8")) as {
  version: string;
};
const outfile = join(outdir, process.platform === "win32" ? "zcode-backend.exe" : "zcode-backend");

await mkdir(join(outdir, "provider"), { recursive: true });
const started = performance.now();
const result = await Bun.build({
  // The codemode worker is a second embedded entrypoint beside main.ts; the runtime starts it
  // as "./codemode-worker.js" (core/src/tool/codemode/sandbox.ts).
  entrypoints: [
    join(root, "apps/zcode-cli/packages/cli/src/main.ts"),
    join(root, "apps/zcode-cli/packages/cli/src/codemode-worker.ts"),
  ],
  compile: { outfile },
  target: "bun",
  format: "esm",
  bytecode: true,
  // Identifier minification stays off: diagnostics and registries rely on function names.
  minify: { whitespace: true, syntax: true, identifiers: false },
  sourcemap: "none",
  // fff stays bundled here: Bun embeds its native library (fff_c.dll and friends) into the
  // executable for the built-in FindFiles/SearchCode tools.
  external: resolveBuildExternal().filter((name) => name !== "@ff-labs/fff-bun"),
  define: {
    __CLI_VERSION__: JSON.stringify(version),
    __ZCODE_VERSION__: JSON.stringify(version),
    // fff-bun only embeds the Linux library when it knows the libc flavour.
    ...(process.platform === "linux" ? { FFF_LIBC: JSON.stringify("gnu") } : {}),
  },
});
if (!result.success) {
  for (const log of result.logs) console.error(log);
  process.exit(1);
}
await copyFile(
  join(root, "config/provider/zcode-builtin.json"),
  join(outdir, "provider/zcode-builtin.json"),
);
const ripgrepEntry = Bun.resolveSync("ripgrep", join(root, "apps/zcode-cli/packages/adapters"));
const ripgrepPackage = dirname(dirname(ripgrepEntry));
const ripgrepTarget = join(outdir, "node_modules/ripgrep");
await rm(ripgrepTarget, { recursive: true, force: true });
await mkdir(ripgrepTarget, { recursive: true });
await copyFile(join(ripgrepPackage, "package.json"), join(ripgrepTarget, "package.json"));
await copyFile(join(ripgrepPackage, "LICENSE.md"), join(ripgrepTarget, "LICENSE.md"));
await cp(join(ripgrepPackage, "lib"), join(ripgrepTarget, "lib"), { recursive: true });
// codemode loads QuickJS from <exe dir>/node_modules/quickjs-wasi/quickjs.wasm. Resolve it
// from pi-codemode, which pins its own quickjs-wasi version.
const codemodeEntry = Bun.resolveSync(
  "@earendil-works/pi-codemode",
  join(root, "apps/zcode-cli/packages/core"),
);
const quickjsWasm = Bun.resolveSync("quickjs-wasi/quickjs.wasm", dirname(codemodeEntry));
const quickjsTarget = join(outdir, "node_modules/quickjs-wasi");
await rm(quickjsTarget, { recursive: true, force: true });
await mkdir(quickjsTarget, { recursive: true });
await copyFile(quickjsWasm, join(quickjsTarget, "quickjs.wasm"));
await copyFile(join(dirname(quickjsWasm), "LICENSE"), join(quickjsTarget, "LICENSE"));
const size = Bun.file(outfile).size / 1024 / 1024;
console.log(
  `Built ${outfile} (${size.toFixed(1)} MiB, v${version}) in ${((performance.now() - started) / 1000).toFixed(1)}s`,
);
