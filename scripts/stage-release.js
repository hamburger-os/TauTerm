import { cpSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const [platform, outputArg = "release-stage"] = process.argv.slice(2);
const output = resolve(root, outputArg);
const bundleRoot = resolve(root, "src-tauri/target/release/bundle");

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
    suffixes: ["_aarch64.dmg", "_aarch64.app.tar.gz", "_aarch64.app.tar.gz.sig"],
    expected: 3,
  },
  "macos-intel": {
    suffixes: ["_x64.dmg", "_x64.app.tar.gz", "_x64.app.tar.gz.sig"],
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

const allFiles = walk(bundleRoot);
if (platform === "windows") {
  smokeTestWindowsInstaller(allFiles);
}

const selected = allFiles.filter((file) => matchesAny(basename(file), spec.suffixes));
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
