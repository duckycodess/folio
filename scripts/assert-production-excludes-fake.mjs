#!/usr/bin/env node
/**
 * The browser-journey fake native core must never reach a build.
 *
 * It lives under `e2e/` and nothing in `src/` imports it, so this is a
 * mechanical check of that fact rather than a claim about it: every file in
 * `dist/` is searched for the fake's sentinel, for the webview globals it
 * installs, and for its own filename.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

const DIST = "dist";
const FORBIDDEN = [
  "folio-e2e-fake-native-core",
  "installFakeNativeCore",
  "__folioFake",
  "__TAURI_INTERNALS__ =",
  "nativeCore.ts",
];

function files(directory) {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

let built;
try {
  built = files(DIST);
} catch {
  console.error(`No ${DIST}/ to check. Run \`npm run build\` first.`);
  process.exit(1);
}

const found = [];
for (const path of built) {
  const text = readFileSync(path, "latin1");
  for (const needle of FORBIDDEN)
    if (text.includes(needle)) found.push(`${path}: ${needle}`);
}

if (found.length) {
  console.error("Test-only code reached the production build:");
  for (const line of found) console.error(`  ${line}`);
  process.exit(1);
}

console.log(
  `${DIST}/: ${built.length} files checked, no browser-journey fake present.`,
);
