import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  appendFile,
  copyFile,
  mkdir,
  readFile,
  readdir,
  stat,
  writeFile,
} from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { arch, release, version } from "node:os";

const args = process.argv.slice(2);
const begin = args[0] === "--begin";
const [target, bundle, outputDir, versionsDir] = begin
  ? [args[1], args[2], null, args[3]]
  : args;
const supported = new Map([
  [
    "x86_64-pc-windows-msvc",
    { bundle: "nsis", extension: ".exe", signing: "unsigned" },
  ],
  [
    "aarch64-apple-darwin",
    {
      bundle: "dmg",
      extension: ".dmg",
      signing: "ad-hoc; no Apple identity or notarization",
    },
  ],
]);
const expected = supported.get(target);
if (
  !expected ||
  bundle !== expected.bundle ||
  (!begin && !outputDir) ||
  !versionsDir
) {
  throw new Error(
    "Usage: collect-packaging.mjs [--begin] <supported-target> <nsis|dmg> [<output-dir>] <versions-dir>",
  );
}
const root = process.cwd();
const source = path.join(
  root,
  "src-tauri",
  "target",
  target,
  "release",
  "bundle",
  bundle,
);
const commit = execFileSync("git", ["rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
const markerPath = path.join(versionsDir, "folio-packaging-start.json");
let entries;
try {
  entries = await readdir(source, { withFileTypes: true });
} catch (error) {
  if (error.code !== "ENOENT") throw error;
  entries = [];
}
const installers = [];
for (const entry of entries) {
  if (entry.isFile() && entry.name.endsWith(expected.extension))
    installers.push(entry.name);
}
installers.sort();
if (begin) {
  if (installers.length !== 0) {
    throw new Error(
      "Existing installers cannot be attributed to a new build. Use a fresh target directory.",
    );
  }
  await writeFile(
    markerPath,
    `${JSON.stringify({ schemaVersion: 1, commit, target, bundle, startedAt: Date.now() })}\n`,
    { flag: "wx" },
  );
  process.exit(0);
}
const marker = JSON.parse(await readFile(markerPath, "utf8"));
if (
  marker.schemaVersion !== 1 ||
  marker.commit !== commit ||
  marker.target !== target ||
  marker.bundle !== bundle ||
  !Number.isFinite(marker.startedAt) ||
  marker.startedAt <= 0 ||
  marker.startedAt > Date.now()
) {
  throw new Error("Build marker does not match this commit and target.");
}
if (installers.length !== 1)
  throw new Error(
    `Expected exactly one fresh ${bundle} installer; found ${installers.length}.`,
  );
const markerMtime = (await stat(markerPath)).mtimeMs;
for (const name of installers) {
  if ((await stat(path.join(source, name))).mtimeMs < markerMtime) {
    throw new Error(`Installer ${name} predates this build marker.`);
  }
}

async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}
await mkdir(outputDir);
const artifacts = [];
for (const name of installers) {
  const file = path.join(source, name);
  const digest = await sha256(file);
  artifacts.push({
    file: name,
    bytes: (await stat(file)).size,
    sha256: digest,
  });
  await copyFile(file, path.join(outputDir, name));
}
const tools = {};
for (const tool of ["node", "npm", "rustc", "cargo", "tauri"]) {
  tools[tool] = (
    await readFile(path.join(versionsDir, `${tool}-version.txt`), "utf8")
  ).trim();
}
const packageJson = JSON.parse(
  await readFile(path.join(root, "package.json"), "utf8"),
);
const metadata = {
  schemaVersion: 1,
  purpose: "installer preparation; not release acceptance",
  commit,
  buildStartedAt: new Date(marker.startedAt).toISOString(),
  appVersion: packageJson.version,
  target,
  bundle,
  runner: {
    os: process.env.RUNNER_OS ?? null,
    arch: process.env.RUNNER_ARCH ?? null,
    processArch: arch(),
    osRelease: release(),
    osVersion: version(),
  },
  run: {
    id: process.env.GITHUB_RUN_ID ?? null,
    attempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
    url:
      process.env.GITHUB_SERVER_URL &&
      process.env.GITHUB_REPOSITORY &&
      process.env.GITHUB_RUN_ID
        ? `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`
        : null,
  },
  signing: expected.signing,
  tools,
  inputSha256: {
    npmLock: await sha256(path.join(root, "package-lock.json")),
    cargoLock: await sha256(path.join(root, "src-tauri", "Cargo.lock")),
    modelManifest: await sha256(
      path.join(root, "src-tauri", "resources", "model-manifest.json"),
    ),
    packagingConfig: await sha256(
      path.join(root, "src-tauri", "tauri.packaging.conf.json"),
    ),
    iconArtwork: await sha256(
      path.join(root, "src-tauri", "icons", "source.png"),
    ),
  },
  artifacts,
  verification: {
    installerBuild: "produced by this run",
    cleanInstallation: "not run",
    desktopBoot: "not run",
    folderPicker: "not run",
    modelSetup: "not run on a packaged build",
    nativeOfflineJourneys: "not run",
    installedFootprint: "not measured",
    targetMemoryAndResponsiveness: "not measured",
    updateAndRemoval: "not run",
  },
};
await writeFile(
  path.join(outputDir, "build-metadata.json"),
  `${JSON.stringify(metadata, null, 2)}\n`,
);
await copyFile(
  path.join(root, "LICENSE"),
  path.join(outputDir, "Folio-LICENSE.txt"),
);
await writeFile(
  path.join(outputDir, "SHA256SUMS.txt"),
  `${artifacts.map(({ file, sha256: hash }) => `${hash}  ${file}`).join("\n")}\n`,
);
const notice = `# Folio installer preparation\n\nTarget: ${target}. Signing: ${expected.signing}.\n\nThese are test artifacts, not a verified consumer release. No model weights or llama.cpp runtime archives were downloaded or added by this packaging workflow. The native build can download ONNX Runtime through ort-sys. Installer creation does not prove required runtime libraries were shipped correctly. First-run model setup, installation, desktop boot, native folder access, offline journeys, size/RAM budgets and update/removal remain pending. Check docs/setup.md and docs/acceptance.md at the recorded commit before testing. Do not disable platform security protections to open a blocked artifact.\n`;
await writeFile(path.join(outputDir, "PREPARATION-NOTICE.md"), notice);
if (process.env.GITHUB_STEP_SUMMARY) {
  await appendFile(
    process.env.GITHUB_STEP_SUMMARY,
    `## Installer preparation: ${target}\n\n${expected.signing}. No installation/offline/budget verification performed.\n\n${artifacts.map(({ file, bytes, sha256: hash }) => `- ${file}: ${bytes} bytes; SHA-256 ${hash}`).join("\n")}\n`,
  );
}
