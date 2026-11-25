#!/usr/bin/env node
// Builds a script-style AIPP plugin (global scripts, no import/export) into a single dist/main.js.
//
// TypeScript 6+ removed `outFile` and `module: none`, which these plugins used to concatenate their
// sources. tsc still type-checks and emits each file into dist/.build; this script then joins them in
// the tsconfig `files` order (what outFile did) and removes the intermediate folder.
//
// Usage (from a plugin's package.json): node ../../scripts/build-script-plugin.mjs
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pluginDir = path.resolve(process.argv[2] ?? process.cwd());
const tsconfig = JSON.parse(fs.readFileSync(path.join(pluginDir, "tsconfig.json"), "utf8"));
const options = tsconfig.compilerOptions ?? {};
const outDir = path.join(pluginDir, options.outDir ?? "dist/.build");
const rootDir = path.join(pluginDir, options.rootDir ?? "src");
const output = path.join(pluginDir, "dist", "main.js");

fs.rmSync(outDir, { recursive: true, force: true });
execFileSync(process.execPath, [path.join(repoRoot, "node_modules/typescript/bin/tsc"), "-p", path.join(pluginDir, "tsconfig.json")], {
    cwd: pluginDir,
    stdio: "inherit",
});

const sources = (tsconfig.files ?? []).filter((file) => !file.endsWith(".d.ts"));
if (!sources.length) throw new Error(`${pluginDir}: tsconfig.json must list the plugin sources in "files", in load order`);
const parts = sources.map((file) => {
    const relative = path.relative(rootDir, path.join(pluginDir, file)).replace(/\.tsx?$/, ".js");
    // TS 6+ always emits "use strict" (alwaysStrict=false was removed). The released plugins ran as
    // sloppy-mode scripts, and a directive at the top would switch the whole joined file to strict mode.
    return fs.readFileSync(path.join(outDir, relative), "utf8").replace(/^"use strict";\r?\n/, "").trimEnd();
});
fs.mkdirSync(path.dirname(output), { recursive: true });
// Type-only files emit nothing; skip them so the output matches what outFile produced.
fs.writeFileSync(output, `${parts.filter(Boolean).join("\n")}\n`, "utf8");
fs.rmSync(outDir, { recursive: true, force: true });
console.log(`Built ${path.relative(repoRoot, output)} from ${sources.length} file(s)`);
