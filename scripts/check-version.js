import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { assertReleaseBoundary } from "./release-changelog.mjs";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const releaseMode = args.includes("--release");
const explicitVersion = args.find((arg) => !arg.startsWith("--"));

function readJson(path) {
  return JSON.parse(readFileSync(resolve(root, path), "utf8"));
}

function fail(message) {
  console.error(`❌ ${message}`);
  process.exitCode = 1;
}

function same(label, actual, expected) {
  if (actual !== expected) {
    fail(`${label}: expected ${expected}, found ${actual}`);
  } else {
    console.log(`✅ ${label}: ${actual}`);
  }
}

const pkg = readJson("package.json");
const expected = (explicitVersion ?? pkg.version).replace(/^v/, "");

if (!/^\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(expected)) {
  fail(
    `unsupported version: ${expected}; use X.Y.Z or X.Y.Z-alpha.N / beta.N / rc.N`,
  );
  process.exit(1);
}

same("package.json", pkg.version, expected);

const lock = readJson("package-lock.json");
same("package-lock.json", lock.version, expected);
same('package-lock.json packages[""]', lock.packages?.[""]?.version, expected);

const tauri = readJson("src-tauri/tauri.conf.json");
same("src-tauri/tauri.conf.json", tauri.version, expected);

const cargo = readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8");
const cargoVersion = cargo.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (!cargoVersion) {
  fail("src-tauri/Cargo.toml: package version not found");
} else {
  same("src-tauri/Cargo.toml", cargoVersion, expected);
}

const cargoLock = readFileSync(resolve(root, "src-tauri/Cargo.lock"), "utf8");
const cargoLockVersion = cargoLock.match(
  /^\[\[package\]\]\s*\nname\s*=\s*"tauterm"\s*\nversion\s*=\s*"([^"]+)"/m,
)?.[1];
if (!cargoLockVersion) {
  fail("src-tauri/Cargo.lock: tauterm package version not found");
} else {
  same("src-tauri/Cargo.lock", cargoLockVersion, expected);
}

if (releaseMode) {
  const changelog = readFileSync(resolve(root, "CHANGELOG.md"), "utf8");
  try {
    assertReleaseBoundary(changelog, expected);
    console.log(
      `✅ CHANGELOG.md: release ${expected} is documented and Unreleased is empty`,
    );
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
}

if (process.exitCode) {
  process.exit(process.exitCode);
}

console.log(`🎉 Version metadata is consistent for ${expected}${releaseMode ? " and release metadata is ready" : ""}.`);
