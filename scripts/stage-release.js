import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const [platform, outputArg = "release-stage"] = process.argv.slice(2);
const output = resolve(root, outputArg);
const bundleRoot = resolve(root, "src-tauri/target/release/bundle");
const { version } = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));

const specs = {
  windows: {
    suffixes: ["_x64-setup.exe", "_x64-setup.exe.sig"],
    expected: 2,
  },
  linux: {
    suffixes: [
      ".deb",
      ".deb.sig",
      ".rpm",
      ".rpm.sig",
      ".AppImage",
      ".AppImage.sig",
    ],
    expected: 6,
  },
  "macos-arm": {
    // Tauri 2.11 keeps the architecture marker on the DMG, but names the
    // updater archive after the app bundle (for example TauTerm.app.tar.gz).
    suffixes: ["_aarch64.dmg", ".app.tar.gz", ".app.tar.gz.sig"],
    expected: 3,
  },
};

function fail(message) {
  throw new Error(message);
}

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

function matchesAny(name, suffixes) {
  return suffixes.some((suffix) => name.endsWith(suffix));
}

function isCurrentVersionArtifact(name) {
  if (platform === "macos-arm" && (name.endsWith(".app.tar.gz") || name.endsWith(".app.tar.gz.sig"))) {
    return true;
  }
  return name.includes(version);
}

function findOne(files, suffix) {
  const matches = files.filter((file) => basename(file).endsWith(suffix));
  if (matches.length !== 1) {
    fail(`Expected exactly one bundle artifact ending with ${suffix}, found ${matches.length}.`);
  }
  return matches[0];
}

function smokeTestWindowsInstaller(files) {
  const installer = findOne(files, "_x64-setup.exe");
  const result = spawnSync("7z", ["l", installer], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (result.error) throw result.error;
  if (result.status !== 0) fail(`7z failed to inspect ${basename(installer)}.`);

  const listing = result.stdout.toLowerCase();
  for (const required of ["tauterm-service.exe", "setupc.exe", "com0com.sys"]) {
    if (!listing.includes(required)) {
      fail(`NSIS installer is missing required bundled file: ${required}`);
    }
  }
}

const spec = specs[platform];
if (!spec) {
  fail(`Unknown release platform ${platform}. Expected one of: ${Object.keys(specs).join(", ")}`);
}
if (typeof version !== "string" || version.length === 0) {
  fail("package.json version is missing or invalid.");
}

const allFiles = walk(bundleRoot);
const currentFiles = allFiles.filter((file) => isCurrentVersionArtifact(basename(file)));
if (platform === "windows") {
  smokeTestWindowsInstaller(currentFiles);
}

const selected = currentFiles.filter((file) => matchesAny(basename(file), spec.suffixes));
for (const suffix of spec.suffixes) {
  findOne(selected, suffix);
}
if (selected.length !== spec.expected) {
  fail(`Expected ${spec.expected} staged artifacts for ${platform}, found ${selected.length}.`);
}

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
const names = new Set();
for (const source of selected) {
  const name = basename(source);
  if (names.has(name)) fail(`Duplicate staged artifact name: ${name}`);
  if (statSync(source).size <= 0) fail(`Bundle artifact is empty: ${name}`);
  names.add(name);
  cpSync(source, join(output, name));
  console.log(`✅ ${name}`);
}

console.log(`🎉 Staged ${selected.length} ${platform} release artifacts in ${output}`);
